-- audioprn: faster fill detection. Only Script Offers are looked at (not every post), and a
-- flair change only checks that post (a new Script Offer can't have been linked to yet).
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

CREATE OR REPLACE FUNCTION audioprn_detect_fill_flair_trigger ()
    RETURNS TRIGGER
    LANGUAGE plpgsql
    AS $$
BEGIN
    PERFORM
        audioprn_detect_fill (coalesce(NEW.post_id, OLD.post_id));
    RETURN NULL;
END
$$;

-- Only an imported Script Offer can be what earlier imported fills link to.
CREATE OR REPLACE FUNCTION audioprn_detect_fill_reddit_trigger ()
    RETURNS TRIGGER
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF audioprn_is_script_offer (NEW.post_id) THEN
        PERFORM
            audioprn_detect_fill (p.id)
        FROM
            post p
        WHERE
            p.body ILIKE '%comments/' || NEW.reddit_id || '%'
            OR p.url ILIKE '%comments/' || NEW.reddit_id || '%';
    END IF;
    RETURN NULL;
END
$$;

