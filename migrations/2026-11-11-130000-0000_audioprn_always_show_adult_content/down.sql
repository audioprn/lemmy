-- Restore upstream defaults. Existing users keep show_nsfw = true / blur_nsfw = false.
ALTER TABLE local_user
    ALTER COLUMN show_nsfw SET DEFAULT FALSE,
    ALTER COLUMN blur_nsfw SET DEFAULT TRUE;
