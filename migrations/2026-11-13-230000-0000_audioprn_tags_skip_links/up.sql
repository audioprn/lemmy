-- audioprn: the text of a markdown link, [text](address), isn't a tag.
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
        regexp_matches(p_name || ' ' || coalesce(p_body, ''), '\[([^][]{1,40})\](?!\()', 'g') AS m;
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
