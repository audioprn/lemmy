# audioprn customisations

Everything specific to this fork lives here, so upstream Lemmy updates merge cleanly.

| Path | What it is |
|---|---|
| `content/` | Site text: 18+ popup, sidebar, community rules, terms & privacy (draft) |
| `setup/phase1.ps1` | Applies Phase 1 site settings through the API. Safe to re-run |
| `dev/` | Local dev config: adds a Mailpit email catcher so signup verification works |

## Run locally

Needs Docker Desktop. From the repo root:

```bash
docker compose -f docker/docker-compose.yml -f audioprn/dev/docker-compose.override.yml up -d --build
```

Before the first build, fetch the email translations submodule. Without it the backend build
fails silently (the Dockerfile ignores the cargo error) and ships an empty placeholder binary
that exits immediately:

```bash
git submodule update --init --recursive
```

The first build of the backend takes a while. Then:

- Site: http://localhost:8880 (admin `lemmy` / `lemmylemmy`)
- Email inbox: http://localhost:8025

**Local testing only.** `audioprn/dev/lemmy.hjson` and the compose files hold default passwords and
keys that are public in this repository. The real site needs its own config with new secrets, kept
out of git.

Apply the Phase 1 settings:

```bash
powershell -File audioprn/setup/phase1.ps1 -AdminPassword lemmylemmy
```

Login, signup, email verification and password reset share one rate limit: 10 requests per
hour per IP address. Running the setup script uses one each time. If local testing hits
`too_many_requests`, `docker restart docker-lemmy-1` resets the counters.

## Phase 1 settings

- **18+ popup** on first visit (site content warning) and every community marked NSFW
- **Email required** and verified at signup
- **Guest access**: logged-out visitors can read after accepting the 18+ popup, but must sign up to comment, vote, follow or post. To require login for everything, run the script with `-PrivateInstance $true`
- **Federation off**, only admins create communities, admins emailed on new reports
- **No video uploads**, and no link thumbnails stored. Images are still allowed for avatars and banners
- One community, `main`, with **Audio** and **Script** post tags
- **Only verified creators and admins can post** (enforced by the backend, see "Verified creators" below)

## Legal text is a draft

`content/legal.md` is shown publicly on the signup and legal pages, so keep notes out of it
(HTML comments are rendered as visible text). It is **not legal advice**. Before launch, have a
lawyer review it, in particular: age-verification duties where you and your users are,
DMCA / notice-and-takedown, GDPR / UK GDPR if you have EU or UK users, and your operator details.
Replace everything in `[brackets]`.

## Verified creators

Code: `crates/audioprn` (backend) and `../lemmy-ui/src/shared/audioprn` (website).

- **Two signup forms.** `/signup` is the simple listener form. `/signup/creator` adds exactly
  3 links to the creator's audios or scripts, plus optional social media links: one input to start, "Add another link" for more (backend cap: 20).
- A creator signup creates a **creator application**. Admins review it at
  `/creator_applications` (also in the admin's user menu) and approve, deny or later revoke it.
- **Only approved creators and admins can create posts.** The website hides "Create post", cross-post and the post form from everyone else. They can still comment,
  vote and follow.
- API, under `/api/v4/audioprn`: `POST /creator_application` (apply or re-apply after a denial),
  `GET /creator_application/mine`, and for admins `GET /creator_application/list?pending_only=`
  and `PUT /creator_application/approve`. `GET /verified_creators` lists verified creator ids.
- Table `creator_application`, created by migration `2026-11-11-120000-0000_audioprn_creator_application`.

## No NSFW marking

The whole site is adult content, so the website has no NSFW labels, badges, blurring or
settings (`enableNsfw()` returns false and `nsfwCheck()` always passes in `lemmy-ui`).
Migration `2026-11-11-130000-0000_audioprn_always_show_adult_content` sets every account to
show adult content unblurred, and makes that the default for new accounts.
