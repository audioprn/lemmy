-- audioprn: the pinned post at the top of a creator's page: social links, terms of use,
-- boundaries and masterlists (the last three in markdown).
CREATE TABLE creator_pin (
    person_id int PRIMARY KEY REFERENCES person ON UPDATE CASCADE ON DELETE CASCADE,
    social_links text[] NOT NULL DEFAULT '{}',
    terms text NOT NULL DEFAULT '',
    boundaries text NOT NULL DEFAULT '',
    masterlists text NOT NULL DEFAULT '',
    updated_at timestamptz NOT NULL DEFAULT now()
);

