DROP TABLE credited_post_hidden;

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
        LEFT JOIN person_actions f ON f.person_id = me
            AND f.target_id = p.creator_id
            AND f.followed_at IS NOT NULL
    WHERE
        p.id = p_id
$$;
