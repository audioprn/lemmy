-- audioprn: "My audioprn", each user's feed: posts from creators they follow (the past 7 days
-- first), then Content posts that meet their tag preferences. Excluded tags hide any post.
-- post_filter_tags.tags keeps every bracket tag (upper case) for favourite / excluded tags.
ALTER TABLE post_filter_tags
    ADD COLUMN tags text[] NOT NULL DEFAULT '{}';

CREATE OR REPLACE FUNCTION audioprn_set_post_filter_tags (p_id int, p_name text, p_body text)
    RETURNS void
    LANGUAGE plpgsql
    AS $$
DECLARE
    tags text[];
    t text;
    voices text := '';
    listeners text := '';
BEGIN
    SELECT
        coalesce(array_agg(upper(trim(m[1]))), '{}') INTO tags
    FROM
        regexp_matches(p_name || ' ' || coalesce(p_body, ''), '\[([^][]{1,40})\]', 'g') AS m;
    FOREACH t IN ARRAY tags LOOP
        IF t ~ '^((TF|TM|NB|F|M|A)+4)+(TF|TM|NB|F|M|A)+$' THEN
            voices := voices || ' ' || regexp_replace(t, '4[^4]*$', '');
            listeners := listeners || ' ' || substring(t FROM '[^4]*$');
        END IF;
    END LOOP;
    INSERT INTO post_filter_tags
        VALUES (p_id, voices ~ '[FA]', voices ~ '[MA]', listeners ~ '[FA]', listeners ~ '[MA]', 'RAPE' = ANY (tags), 'INCEST' = ANY (tags), EXISTS (
                SELECT
                    1
                FROM
                    taboo_tag
                WHERE
                    upper(tag) = ANY (tags)), tags)
    ON CONFLICT (post_id)
        DO UPDATE SET
            voice_f = excluded.voice_f, voice_m = excluded.voice_m, listener_f = excluded.listener_f, listener_m = excluded.listener_m, rape = excluded.rape, incest = excluded.incest, taboo = excluded.taboo, tags = excluded.tags;
END
$$;

SELECT
    audioprn_set_post_filter_tags (id, name, body)
FROM
    post;

-- A user's My audioprn preferences. No row = the defaults.
CREATE TABLE feed_prefs (
    person_id int PRIMARY KEY REFERENCES person ON UPDATE CASCADE ON DELETE CASCADE,
    f_listener boolean NOT NULL DEFAULT TRUE,
    m_listener boolean NOT NULL DEFAULT TRUE,
    f_voices boolean NOT NULL DEFAULT FALSE,
    m_voices boolean NOT NULL DEFAULT FALSE,
    scripts boolean NOT NULL DEFAULT FALSE,
    audios boolean NOT NULL DEFAULT FALSE,
    favourite_tags text[] NOT NULL DEFAULT '{}',
    excluded_tags text[] NOT NULL DEFAULT '{}'
);

-- Where a post goes in a user's feed: 'recent' (followed creator, past 7 days), 'feed', or NULL
-- (not in the feed). Followed creators' posts skip the tag preferences, but not excluded tags.
CREATE FUNCTION audioprn_my_feed (p_id int, me int)
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
        LEFT JOIN person_actions f ON f.person_id = me
            AND f.target_id = p.creator_id
            AND f.followed_at IS NOT NULL
    WHERE
        p.id = p_id
$$;
