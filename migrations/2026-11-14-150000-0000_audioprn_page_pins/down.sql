ALTER TABLE creator_pin
    ADD COLUMN masterlists text NOT NULL DEFAULT '';

DROP TABLE creator_page_pin;

