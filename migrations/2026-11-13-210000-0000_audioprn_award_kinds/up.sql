-- audioprn: award types (Goat, Kino, Dropped Crown, Uwu, Respect). Earlier awards become Goat.
ALTER TABLE post_award
    ADD COLUMN kind text NOT NULL DEFAULT 'goat' CHECK (kind IN ('goat', 'kino', 'dropped_crown', 'uwu', 'respect'));

ALTER TABLE post_award
    ALTER COLUMN kind DROP DEFAULT;

