-- audioprn: admins make awards (the five built-in ones are emoji; new ones are an uploaded image)
-- and set how many awards people get. Awards are a balance that never expires: everyone starts
-- with start_awards, and gets one more every every_days days (counted from joining) when they
-- were active in the window_days before: min_comments comments or reactions to min_reactions
-- posts (0 turns that way off). Minus the awards they gave.
CREATE SEQUENCE award_type_seq;

CREATE TABLE award_type (
    kind text PRIMARY KEY DEFAULT 'award_' || nextval('award_type_seq') CHECK (kind ~ '^[a-z0-9_]{1,40}$'),
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 40),
    emoji text CHECK (char_length(emoji) <= 16),
    -- A small picture as a data URL (no SVG).
    image text CHECK (length(image) <= 150000 AND image ~ '^data:image/(png|jpeg|gif|webp);base64,[A-Za-z0-9+/]+=*$'),
    position serial NOT NULL,
    -- Hidden awards can't be given any more; ones already given still show.
    hidden boolean NOT NULL DEFAULT FALSE,
    CHECK ((emoji IS NULL) <> (image IS NULL))
);

INSERT INTO award_type (kind, name, emoji)
    VALUES ('goat', 'Goat', '🐐'),
    ('kino', 'Kino', '🎞️'),
    ('dropped_crown', 'Dropped Crown', '👑'),
    ('uwu', 'Uwu', '🥹'),
    ('respect', 'Respect', '🫡');

ALTER TABLE post_award
    DROP CONSTRAINT post_award_kind_check,
    ADD FOREIGN KEY (kind) REFERENCES award_type ON UPDATE CASCADE,
    DROP COLUMN pool;

CREATE TABLE award_settings (
    id int PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    start_awards int NOT NULL DEFAULT 20 CHECK (start_awards BETWEEN 0 AND 1000),
    every_days int NOT NULL DEFAULT 3 CHECK (every_days BETWEEN 1 AND 365),
    window_days int NOT NULL DEFAULT 7 CHECK (window_days BETWEEN 1 AND 365),
    min_comments int NOT NULL DEFAULT 1 CHECK (min_comments BETWEEN 0 AND 1000),
    min_reactions int NOT NULL DEFAULT 10 CHECK (min_reactions BETWEEN 0 AND 1000)
);

INSERT INTO award_settings DEFAULT VALUES;

-- For counting a person's recent reactions.
CREATE INDEX idx_post_reaction_person ON post_reaction (person_id, published_at);

-- How many awards a person can give now.
CREATE FUNCTION audioprn_awards_left (pid int)
    RETURNS bigint
    LANGUAGE sql
    STABLE
    AS $$
    SELECT
        greatest (0, s.start_awards + (
                SELECT
                    count(*)
                FROM generate_series(p.published_at + make_interval(days => s.every_days), now(), make_interval(days => s.every_days)) b
                WHERE (s.min_comments > 0
                    AND (
                        SELECT
                            count(*)
                        FROM comment c
                        WHERE c.creator_id = pid AND NOT c.deleted AND NOT c.removed
                            AND c.published_at > b - make_interval(days => s.window_days)
                            AND c.published_at <= b) >= s.min_comments)
                    OR (s.min_reactions > 0
                        AND (
                            SELECT
                                count(*)
                            FROM post_reaction r
                            WHERE r.person_id = pid
                                AND r.published_at > b - make_interval(days => s.window_days)
                                AND r.published_at <= b) >= s.min_reactions)) - (
                SELECT
                    count(*)
                FROM post_award
                WHERE giver_id = pid))
    FROM person p, award_settings s
    WHERE p.id = pid
$$;
