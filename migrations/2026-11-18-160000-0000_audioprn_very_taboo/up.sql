-- audioprn: very taboo tags (admins' list, lower case without brackets) can only be posted in the
-- cummunities admins allow (by name, so it holds before they're created) and on creator pages, and
-- never show on the Content page (except in searches that ask for them).
CREATE TABLE very_taboo_tag (
    tag text PRIMARY KEY CHECK (tag = lower(tag))
);

INSERT INTO very_taboo_tag (tag)
    VALUES ('extreme'), ('racial slurs'), ('gender slurs'), ('orientation slurs');

CREATE TABLE very_taboo_community (
    name text PRIMARY KEY
);

INSERT INTO very_taboo_community (name)
    VALUES ('dark');
