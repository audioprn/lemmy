-- audioprn: creators' own news flashes, shown on their profile and in their followers' My audioprn.
ALTER TABLE news_flash
    ADD COLUMN person_id int REFERENCES person ON UPDATE CASCADE ON DELETE CASCADE,
    ADD CONSTRAINT news_flash_one_owner CHECK (community_id IS NULL OR person_id IS NULL);

CREATE INDEX idx_news_flash_person ON news_flash (person_id, published_at DESC)
WHERE
    person_id IS NOT NULL;
