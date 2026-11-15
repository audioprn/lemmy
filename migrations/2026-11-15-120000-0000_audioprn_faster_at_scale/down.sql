DROP FUNCTION audioprn_in_my_feed (int, int, text);

DROP MATERIALIZED VIEW audioprn_popular_tags;

DROP INDEX idx_post_creator_published, idx_post_url_trgm;

DROP INDEX idx_post_script_link;

CREATE OR REPLACE FUNCTION audioprn_detect_fill (pid int)
    RETURNS void
    LANGUAGE plpgsql
    AS $$
DECLARE
    txt text;
    linked int[];
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
        SELECT
            min(so.id) INTO found
        FROM
            post_community_tag pct
            JOIN community_tag ct ON ct.id = pct.community_tag_id
            JOIN post so ON so.id = pct.post_id
        WHERE
            ct.name = 'script_offer'
            AND NOT ct.deleted
            AND so.id <> pid
            AND NOT so.deleted
            AND (so.id = ANY (linked)
                OR (so.url IS NOT NULL
                    AND position(so.url IN txt) > 0)
                OR EXISTS (
                    SELECT
                        1
                    FROM
                        post_script ps
                    WHERE
                        ps.post_id = so.id
                        AND ps.link IS NOT NULL
                        AND position(ps.link IN txt) > 0));
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
                            JOIN person pe ON lower(pc.name) = 'u/' || lower(pe.name)
                        WHERE
                            pc.post_id = p.id))
            LIMIT 1) f ON TRUE
    WHERE
        p.id = p_id
$$;
