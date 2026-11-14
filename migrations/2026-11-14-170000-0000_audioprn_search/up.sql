-- audioprn: gwasi-style search (see search_sql in lemmy/crates/db_views/post/src/audioprn.rs).
-- Helpers for the search, script fills (which Script Offer a post fills, for fills: and
-- userfills:), and each user's saved search.
--
-- Text as a case-insensitive "contains" pattern for ILIKE.
CREATE FUNCTION audioprn_like (t text)
    RETURNS text
    LANGUAGE sql
    IMMUTABLE
    AS $$
    SELECT
        '%' || replace(replace(replace(t, '\', '\\'), '%', '\%'), '_', '\_') || '%'
$$;

-- A case-insensitive regex match; a broken pattern matches nothing instead of failing. \b (a
-- word boundary, as on gwasi) is \y in Postgres.
CREATE FUNCTION audioprn_regex_match (t text, pattern text)
    RETURNS boolean
    LANGUAGE plpgsql
    IMMUTABLE
    AS $$
BEGIN
    RETURN t ~* replace(pattern, '\b', '\y');
EXCEPTION
    WHEN OTHERS THEN
        RETURN FALSE;
END
$$;

-- Minutes from a length like 16:10, 1:02:03, 28m, 22m 52s, 1h 05m or 85 minutes (else null).
CREATE FUNCTION audioprn_minutes (t text)
    RETURNS numeric
    LANGUAGE sql
    IMMUTABLE
    AS $$
    SELECT
        CASE WHEN clock IS NOT NULL THEN
            coalesce(clock[1]::numeric, 0) * 60 + clock[2]::numeric + clock[3]::numeric / 60
        WHEN words IS NOT NULL AND (words[1] IS NOT NULL OR words[2] IS NOT NULL) THEN
            coalesce(words[1]::numeric, 0) * 60 + coalesce(words[2]::numeric, 0) + coalesce(words[3]::numeric, 0) / 60
        END
    FROM (
        SELECT
            regexp_match(trim(t), '^(?:(\d{1,2}):)?(\d{1,3}):(\d{2})$') AS clock,
            regexp_match(lower(trim(t)), '^(?:(\d+)\s*h(?:ours?|rs?)?)?\s*(?:(\d+)\s*m(?:in(?:ute)?s?)?)?\s*(?:(\d+)\s*s(?:ec(?:ond)?s?)?)?$') AS words) x
$$;

-- A post's length for length: minutes for audios (from a length tag in the title, else the
-- creator's masterlist), thousands of words for Script Offers with the script on the site.
CREATE FUNCTION audioprn_post_length (pid int)
    RETURNS numeric
    LANGUAGE sql
    STABLE
    AS $$
    SELECT
        CASE WHEN EXISTS (
            SELECT
                1
            FROM
                post_community_tag pct
                JOIN community_tag ct ON ct.id = pct.community_tag_id
            WHERE
                pct.post_id = pid
                AND ct.name = 'script_offer') THEN
        (
            SELECT
                round(array_length(regexp_split_to_array(trim(body), '\s+'), 1) / 1000.0, 1)
            FROM
                post_script
            WHERE
                post_id = pid
                AND trim(body) <> '')
    ELSE
        coalesce((
            SELECT
                audioprn_minutes (tag)
            FROM post_filter_tags t, unnest(t.tags) tag
            WHERE
                t.post_id = pid
                AND audioprn_minutes (tag) IS NOT NULL
            LIMIT 1), (
            SELECT
                audioprn_minutes (duration)
            FROM masterlist_entry
            WHERE
                post_id = pid))
        END
$$;

-- Which Script Offer a post fills. `chosen`: picked by the filler on the post form; otherwise
-- found from a link in the post to the Script Offer (its post here, its script link, or its
-- Reddit post when both were imported from Reddit).
CREATE TABLE script_fill (
    fill_post_id int PRIMARY KEY REFERENCES post ON UPDATE CASCADE ON DELETE CASCADE,
    script_post_id int NOT NULL REFERENCES post ON UPDATE CASCADE ON DELETE CASCADE,
    chosen boolean NOT NULL DEFAULT FALSE,
    CHECK (fill_post_id <> script_post_id)
);

CREATE INDEX idx_script_fill_script ON script_fill (script_post_id);

CREATE FUNCTION audioprn_is_script_offer (pid int)
    RETURNS boolean
    LANGUAGE sql
    STABLE
    AS $$
    SELECT
        EXISTS (
            SELECT
                1
            FROM
                post_community_tag pct
                JOIN community_tag ct ON ct.id = pct.community_tag_id
            WHERE
                pct.post_id = pid
                AND ct.name = 'script_offer'
                AND NOT ct.deleted)
$$;

-- Finds (again) the Script Offer a post links to, unless the filler chose one.
CREATE FUNCTION audioprn_detect_fill (pid int)
    RETURNS void
    LANGUAGE plpgsql
    AS $$
DECLARE
    txt text;
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
    IF txt IS NULL OR audioprn_is_script_offer (pid) THEN
        found := NULL;
    ELSE
        SELECT
            so.id INTO found
        FROM
            post so
        WHERE
            so.id <> pid
            AND NOT so.deleted
            AND audioprn_is_script_offer (so.id)
            AND (so.id IN (
                    SELECT
                        m[1]::int
                    FROM
                        regexp_matches(txt, '/post/(\d{1,9})(?!\d)', 'g') m)
                OR so.id IN (
                    SELECT
                        ri.post_id
                    FROM
                        reddit_import ri
                    WHERE
                        ri.reddit_id IN (
                            SELECT
                                m[1]
                            FROM
                                regexp_matches(txt, 'comments/([a-z0-9]{4,12})', 'gi') m))
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
                            AND position(ps.link IN txt) > 0))
        ORDER BY
            so.id
        LIMIT 1;
    END IF;
    IF found IS NULL THEN
        DELETE FROM script_fill
        WHERE fill_post_id = pid;
    ELSE
        INSERT INTO script_fill (fill_post_id, script_post_id)
            VALUES (pid, found)
        ON CONFLICT (fill_post_id)
            DO UPDATE SET
                script_post_id = excluded.script_post_id;
    END IF;
END
$$;

CREATE FUNCTION audioprn_detect_fill_post_trigger ()
    RETURNS TRIGGER
    LANGUAGE plpgsql
    AS $$
BEGIN
    PERFORM
        audioprn_detect_fill (NEW.id);
    RETURN NULL;
END
$$;

CREATE TRIGGER audioprn_detect_fill
    AFTER INSERT OR UPDATE OF body, url ON post
    FOR EACH ROW
    EXECUTE FUNCTION audioprn_detect_fill_post_trigger ();

-- A post's flair changing (a post becoming a Script Offer or not): check it, and the posts that
-- link to it.
CREATE FUNCTION audioprn_detect_fill_flair_trigger ()
    RETURNS TRIGGER
    LANGUAGE plpgsql
    AS $$
DECLARE
    pid int := coalesce(NEW.post_id, OLD.post_id);
BEGIN
    PERFORM
        audioprn_detect_fill (pid);
    PERFORM
        audioprn_detect_fill (p.id)
    FROM
        post p
    WHERE
        p.body LIKE '%/post/' || pid || '%';
    RETURN NULL;
END
$$;

CREATE TRIGGER audioprn_detect_fill
    AFTER INSERT OR DELETE ON post_community_tag
    FOR EACH ROW
    EXECUTE FUNCTION audioprn_detect_fill_flair_trigger ();

-- A Script Offer imported from Reddit after its fills: link the fills that point to it.
CREATE FUNCTION audioprn_detect_fill_reddit_trigger ()
    RETURNS TRIGGER
    LANGUAGE plpgsql
    AS $$
BEGIN
    PERFORM
        audioprn_detect_fill (p.id)
    FROM
        post p
    WHERE
        p.body ILIKE '%comments/' || NEW.reddit_id || '%'
        OR p.url ILIKE '%comments/' || NEW.reddit_id || '%';
    RETURN NULL;
END
$$;

CREATE TRIGGER audioprn_detect_fill
    AFTER INSERT ON reddit_import
    FOR EACH ROW
    EXECUTE FUNCTION audioprn_detect_fill_reddit_trigger ();

SELECT
    audioprn_detect_fill (id)
FROM
    post;

-- A search the user always adds on the Content page (like gwasi's Persistent Query).
CREATE TABLE search_prefs (
    person_id int PRIMARY KEY REFERENCES person ON UPDATE CASCADE ON DELETE CASCADE,
    saved_query text NOT NULL DEFAULT '' CHECK (char_length(saved_query) <= 1000),
    use_saved boolean NOT NULL DEFAULT TRUE
);

