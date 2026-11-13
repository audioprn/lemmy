-- audioprn: creators can hide their public follower count (shown by default).
ALTER TABLE creator_application
    ADD COLUMN hide_follower_count boolean NOT NULL DEFAULT FALSE;
