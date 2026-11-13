-- audioprn: an optional footer creators add under each of their posts (a blurb and social links),
-- and news flashes for community pages (posted by the community's mods; admins choose which also
-- show in the main news).
ALTER TABLE creator_application
    ADD COLUMN footer_blurb text NOT NULL DEFAULT '' CHECK (char_length(footer_blurb) <= 500),
    ADD COLUMN footer_links text[] NOT NULL DEFAULT '{}' CHECK (cardinality(footer_links) <= 10);

ALTER TABLE news_flash
    ADD COLUMN community_id int REFERENCES community ON UPDATE CASCADE ON DELETE CASCADE,
    ADD COLUMN in_main boolean NOT NULL DEFAULT FALSE;
