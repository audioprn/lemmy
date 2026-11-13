-- audioprn: My audioprn counts a post as from a followed creator when they posted it or are
-- credited on it as u/name (post_credit).
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

-- Credited posts a creator hid from their own profile's "Credited on" list (shown by default).
CREATE TABLE credited_post_hidden (
    person_id int NOT NULL REFERENCES person ON UPDATE CASCADE ON DELETE CASCADE,
    post_id int NOT NULL REFERENCES post ON UPDATE CASCADE ON DELETE CASCADE,
    PRIMARY KEY (person_id, post_id)
);
