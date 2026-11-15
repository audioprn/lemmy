-- audioprn: a creator's optional cash box, shown inside their Content posts at the bottom (their
-- Throne, Ko-fi, Patreon… links, written as markdown).
ALTER TABLE creator_application
    ADD COLUMN support_text text NOT NULL DEFAULT '' CHECK (char_length(support_text) <= 500);
