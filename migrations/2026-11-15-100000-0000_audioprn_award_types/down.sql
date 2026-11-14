DROP FUNCTION audioprn_awards_left;

DROP INDEX idx_post_reaction_person;

DROP TABLE award_settings;

ALTER TABLE post_award
    DROP CONSTRAINT post_award_kind_fkey,
    ADD COLUMN pool text NOT NULL DEFAULT 'weekly' CHECK (pool IN ('weekly', 'monthly'));

DELETE FROM post_award
WHERE kind NOT IN ('goat', 'kino', 'dropped_crown', 'uwu', 'respect');

ALTER TABLE post_award
    ADD CONSTRAINT post_award_kind_check CHECK (kind IN ('goat', 'kino', 'dropped_crown', 'uwu', 'respect'));

DROP TABLE award_type;

DROP SEQUENCE award_type_seq;
