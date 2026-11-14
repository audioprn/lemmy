-- audioprn: listeners' favourite masterlist entries (favourite posts use the site's Save).
CREATE TABLE masterlist_favourite (
    person_id int NOT NULL REFERENCES person ON UPDATE CASCADE ON DELETE CASCADE,
    entry_id int NOT NULL REFERENCES masterlist_entry ON UPDATE CASCADE ON DELETE CASCADE,
    PRIMARY KEY (person_id, entry_id)
);

