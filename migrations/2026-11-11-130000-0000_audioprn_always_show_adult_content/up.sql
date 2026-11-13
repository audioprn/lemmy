-- audioprn: the whole site is adult content and the website has no NSFW settings,
-- so every account shows NSFW content, unblurred. This includes accounts not created through
-- signup, such as the first admin from initial setup.
ALTER TABLE local_user
    ALTER COLUMN show_nsfw SET DEFAULT TRUE,
    ALTER COLUMN blur_nsfw SET DEFAULT FALSE;

UPDATE
    local_user
SET
    show_nsfw = TRUE,
    blur_nsfw = FALSE
WHERE
    show_nsfw = FALSE
    OR blur_nsfw = TRUE;
