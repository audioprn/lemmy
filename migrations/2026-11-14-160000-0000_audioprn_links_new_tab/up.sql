-- audioprn: links to other sites open in a new tab by default (users can turn it off in Settings).
ALTER TABLE local_user
    ALTER COLUMN open_links_in_new_tab SET DEFAULT TRUE;

UPDATE
    local_user
SET
    open_links_in_new_tab = TRUE;

