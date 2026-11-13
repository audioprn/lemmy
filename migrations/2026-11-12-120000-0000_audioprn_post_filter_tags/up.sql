-- audioprn: bracket tags in a post's title and body, read for the content filters.
-- Gender tags like [F4M] = F voice for M listener. TF counts as F, TM as M, A (any) as both.
-- With several parts ([M4F4M]) every part but the last is a voice, the last is the listener.
-- [Rape] and [Incest] match exactly; a post is Taboo when any tag is in taboo_tag.
CREATE TABLE taboo_tag (
    tag text PRIMARY KEY CHECK (tag = lower(tag))
);

INSERT INTO taboo_tag (tag)
    VALUES ('rape'), ('cnc'), ('non-con'), ('dubcon'), ('incest'), ('death'), ('gore'), ('vore'), ('cbt'), ('watersports');

CREATE TABLE post_filter_tags (
    post_id int PRIMARY KEY REFERENCES post ON UPDATE CASCADE ON DELETE CASCADE,
    voice_f boolean NOT NULL,
    voice_m boolean NOT NULL,
    listener_f boolean NOT NULL,
    listener_m boolean NOT NULL,
    rape boolean NOT NULL,
    incest boolean NOT NULL,
    taboo boolean NOT NULL
);

CREATE FUNCTION audioprn_set_post_filter_tags (p_id int, p_name text, p_body text)
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
                    upper(tag) = ANY (tags)))
    ON CONFLICT (post_id)
        DO UPDATE SET
            voice_f = excluded.voice_f, voice_m = excluded.voice_m, listener_f = excluded.listener_f, listener_m = excluded.listener_m, rape = excluded.rape, incest = excluded.incest, taboo = excluded.taboo;
END
$$;

CREATE FUNCTION audioprn_post_filter_tags_trigger ()
    RETURNS TRIGGER
    LANGUAGE plpgsql
    AS $$
BEGIN
    PERFORM
        audioprn_set_post_filter_tags (NEW.id, NEW.name, NEW.body);
    RETURN NULL;
END
$$;

CREATE TRIGGER audioprn_post_filter_tags
    AFTER INSERT OR UPDATE OF name, body ON post
    FOR EACH ROW
    EXECUTE FUNCTION audioprn_post_filter_tags_trigger ();

SELECT
    audioprn_set_post_filter_tags (id, name, body)
FROM
    post;
