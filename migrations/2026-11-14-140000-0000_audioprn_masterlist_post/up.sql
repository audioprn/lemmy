-- audioprn: a masterlist entry can add details to one of the creator's posts (from their
-- spreadsheet: a clean title, description, length, categories), instead of being another audio.
ALTER TABLE masterlist_entry
    ADD COLUMN post_id int REFERENCES post ON UPDATE CASCADE ON DELETE CASCADE;

CREATE UNIQUE INDEX idx_masterlist_entry_post ON masterlist_entry (post_id);

