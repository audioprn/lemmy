ALTER TABLE news_flash
    DROP COLUMN community_id,
    DROP COLUMN in_main;

ALTER TABLE creator_application
    DROP COLUMN footer_blurb,
    DROP COLUMN footer_links;
