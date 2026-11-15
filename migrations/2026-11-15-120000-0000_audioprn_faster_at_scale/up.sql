-- audioprn: fixes from load testing with gwasi's 245k posts.
CREATE INDEX idx_post_script_link ON post_script (link);

CREATE OR REPLACE FUNCTION audioprn_detect_fill (pid int)
    RETURNS void
    LANGUAGE plpgsql
    AS $$
DECLARE
    txt text;
    linked int[];
    links text[];
    found int;
BEGIN
    IF EXISTS (
        SELECT
            1
        FROM
            script_fill
        WHERE
            fill_post_id = pid
            AND chosen) THEN
        RETURN;
    END IF;
    SELECT
        coalesce(body, '') || ' ' || coalesce(url, '') INTO txt
    FROM
        post
    WHERE
        id = pid;
    IF txt IS NOT NULL AND NOT audioprn_is_script_offer (pid) THEN
        -- Posts linked as /post/123, and Reddit posts that were imported here.
        linked := ARRAY (
            SELECT
                m[1]::int
            FROM
                regexp_matches(txt, '/post/(\d{1,9})(?!\d)', 'g') m
            UNION
            SELECT
                ri.post_id
            FROM
                reddit_import ri
            WHERE
                ri.reddit_id IN (
                    SELECT
                        lower(m[1])
                    FROM
                        regexp_matches(txt, 'comments/([a-z0-9]{4,12})', 'gi') m));
        -- Links in the post, with and without a trailing slash.
        links := ARRAY (
            SELECT
                v
            FROM
                regexp_matches(txt, 'https?://[^\s()<>\[\]"''*`]+', 'g') m,
                rtrim(m[1], '/.,;:!?') t,
                unnest(ARRAY[t, t || '/']) v);
        SELECT
            min(so.id) INTO found
        FROM
            post so
        WHERE
            so.id IN (
                SELECT
                    unnest(linked)
                UNION
                SELECT
                    id
                FROM
                    post
                WHERE
                    url = ANY (links)
                UNION
                SELECT
                    post_id
                FROM
                    post_script
                WHERE
                    link = ANY (links))
            AND so.id <> pid
            AND NOT so.deleted
            AND audioprn_is_script_offer (so.id);
    END IF;
    IF found IS NULL THEN
        DELETE FROM script_fill
        WHERE fill_post_id = pid
            AND NOT chosen;
    ELSE
        INSERT INTO script_fill (fill_post_id, script_post_id)
            VALUES (pid, found)
        ON CONFLICT (fill_post_id)
            DO UPDATE SET
                script_post_id = excluded.script_post_id;
    END IF;
END
$$;

-- A Reddit-imported Script Offer looks for fills linking to it (audioprn_detect_fill_reddit_trigger,
-- `url ILIKE '%comments/…%'`): post text already has a trigram index, links didn't.
CREATE INDEX idx_post_url_trgm ON post USING gin (url gin_trgm_ops);

-- Creators page sorted by latest post.
CREATE INDEX idx_post_creator_published ON post (creator_id, published_at DESC)
WHERE
    NOT deleted AND NOT removed;

-- Popular tags (Content search panel), counted hourly by the scheduler instead of on every page
-- view (about 20 seconds with 245k posts).
CREATE MATERIALIZED VIEW audioprn_popular_tags AS
SELECT
    tag,
    count(*) AS count
FROM (
    SELECT DISTINCT
        t.post_id,
        rtrim(tag, '\') AS tag
    FROM
        post_filter_tags t
        JOIN post p ON p.id = t.post_id,
        unnest(t.tags) tag
    WHERE
        NOT p.deleted
        AND NOT p.removed
        AND NOT EXISTS (
            SELECT
                1
            FROM
                post_off_content o
            WHERE
                o.post_id = p.id)
        AND p.community_id NOT IN (
            SELECT
                id
            FROM
                community
            WHERE
                name = 'creators')) x
WHERE
    tag !~ '^\s*\d'
    OR audioprn_minutes (tag) IS NULL
GROUP BY
    1
ORDER BY
    2 DESC,
    1
LIMIT 30;

CREATE UNIQUE INDEX idx_audioprn_popular_tags ON audioprn_popular_tags (tag);

-- My audioprn: credits as u/name are matched to people by index (was a scan of every person for
-- each post).
CREATE OR REPLACE FUNCTION audioprn_my_feed (p_id int, me int)
    RETURNS text
    LANGUAGE sql
    STABLE
    AS $$
    SELECT
        CASE WHEN coalesce(t.tags, '{}') && coalesce(fp.excluded_tags, '{}') THEN
            NULL
        WHEN f.target_id IS NOT NULL
            AND p.published_at > now() - interval '7 days' THEN
            'recent'
        WHEN f.target_id IS NOT NULL THEN
            'feed'
        WHEN c.name = 'creators'
            OR EXISTS (
                SELECT
                    1
                FROM
                    post_off_content o
                WHERE
                    o.post_id = p.id) THEN
            NULL
        WHEN NOT (coalesce(t.listener_f OR t.listener_m, FALSE)
            AND NOT ((coalesce(fp.f_listener, TRUE)
                AND t.listener_f)
            OR (coalesce(fp.m_listener, TRUE)
                AND t.listener_m)))
            AND (NOT coalesce(fp.f_voices, FALSE)
                OR coalesce(t.voice_f, FALSE))
            AND (NOT coalesce(fp.m_voices, FALSE)
                OR coalesce(t.voice_m, FALSE))
            AND ((NOT coalesce(fp.scripts, FALSE)
                    AND NOT coalesce(fp.audios, FALSE))
                OR EXISTS (
                    SELECT
                        1
                    FROM
                        post_community_tag pct
                        JOIN community_tag ct ON ct.id = pct.community_tag_id
                    WHERE
                        pct.post_id = p.id
                        AND NOT ct.deleted
                        AND ((fp.scripts
                                AND ct.name = 'script_offer')
                            OR (fp.audios
                                AND ct.name IN ('script_fill', 'oc')))))
            AND (cardinality(coalesce(fp.favourite_tags, '{}')) = 0
                OR coalesce(t.tags, '{}') && fp.favourite_tags) THEN
            'feed'
        END
    FROM
        post p
        JOIN community c ON c.id = p.community_id
        LEFT JOIN post_filter_tags t ON t.post_id = p.id
        LEFT JOIN feed_prefs fp ON fp.person_id = me
        LEFT JOIN LATERAL (
            SELECT
                f.target_id
            FROM
                person_actions f
            WHERE
                f.person_id = me
                AND f.followed_at IS NOT NULL
                AND (f.target_id = p.creator_id
                    OR f.target_id IN (
                        SELECT
                            pe.id
                        FROM
                            post_credit pc
                            JOIN person pe ON lower(pe.name) = lower(substr(pc.name, 3))
                        WHERE
                            pc.post_id = p.id
                            AND pc.name ILIKE 'u/%'))
            LIMIT 1) f ON TRUE
    WHERE
        p.id = p_id
$$;

-- My audioprn's filter as a yes/no function that's costly to run: compared as text
-- (`audioprn_my_feed(…) = 'feed'`), Postgres guessed almost no posts match, ran it on every post
-- and sorted them all (over a minute with 245k posts) instead of reading newest first.
CREATE FUNCTION audioprn_in_my_feed (p_id int, me int, part text)
    RETURNS boolean
    LANGUAGE plpgsql
    STABLE
    COST 5000
    AS $$
BEGIN
    RETURN audioprn_my_feed (p_id, me) = part;
END
$$;
