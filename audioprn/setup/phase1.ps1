<#
  Phase 1: apply site settings for the 18+ audio & fiction community.
  Safe to re-run: it edits the site, creates the main and creators communities if missing,
  creates missing post flairs and deletes tags that aren't flairs.

  Usage (local dev defaults):
    .\audioprn\setup\phase1.ps1
  Against another instance:
    .\audioprn\setup\phase1.ps1 -BaseUrl https://staging.example.com -AdminUser me
#>
param(
  [string]$BaseUrl = "http://localhost:8880",
  [string]$AdminUser = "lemmy",
  [string]$AdminPassword = "",          # prompted for if empty
  [string]$SiteName = "audioprn",       # max 20 characters
  [string]$CommunityName = "main",
  [string]$CommunityTitle = "Content page",
  # false = guest access: logged-out visitors can read after accepting the 18+ popup, but must
  # sign up to comment, vote, follow or post. true = nothing is visible until you log in.
  [bool]$PrivateInstance = $false
)

$ErrorActionPreference = "Stop"
$content = Join-Path $PSScriptRoot "..\content"
$api = "$BaseUrl/api/v4"

# Plain .NET read: Get-Content adds hidden PS properties that ConvertTo-Json would serialize (hangs on PS 5.1).
function Read-Text($name) { [System.IO.File]::ReadAllText((Resolve-Path (Join-Path $content $name)).Path, [System.Text.Encoding]::UTF8) }

function Invoke-Api($method, $path, $body) {
  $params = @{ Method = $method; Uri = "$api$path"; Headers = $script:headers; ContentType = "application/json; charset=utf-8" }
  if ($null -ne $body) {
    if ($method -eq "GET") { $params.Uri += "?" + (($body.GetEnumerator() | ForEach-Object { "$($_.Key)=$([uri]::EscapeDataString([string]$_.Value))" }) -join "&") }
    else { $params.Body = [System.Text.Encoding]::UTF8.GetBytes(($body | ConvertTo-Json -Depth 10)) }
  }
  Invoke-RestMethod @params
}

# --- Log in ---------------------------------------------------------------
if (-not $AdminPassword) {
  $secure = Read-Host "Password for admin '$AdminUser'" -AsSecureString
  $AdminPassword = [System.Net.NetworkCredential]::new("", $secure).Password
}
$script:headers = @{}
$login = Invoke-Api POST "/account/auth/login" @{ username_or_email = $AdminUser; password = $AdminPassword }
$script:headers = @{ Authorization = "Bearer $($login.jwt)" }
Write-Host "Logged in as $AdminUser"

# --- Site settings ----------------------------------------------------------
$site = @{
  name                          = $SiteName
  summary                       = "An 18+ cummunity for sharing and discussing erotic audio and fiction."
  sidebar                       = Read-Text "sidebar.md"
  legal_information             = Read-Text "legal.md"
  # Shown as a popup to every visitor before they can use the site (the 18+ confirmation).
  content_warning               = Read-Text "content-warning.md"
  # Accounts need a verified email address. Requires the email section in lemmy.hjson.
  email_verification_required   = $true
  registration_mode             = "open"
  private_instance              = $PrivateInstance
  # Single closed community: no federation with other Lemmy servers.
  federation_enabled            = $false
  community_creation_admin_only = $true
  reports_email_admins          = $true
  # Audio and scripts are linked, never hosted. No images except avatars (everyone) and banners
  # (verified creators, checked in crates/audioprn): uploads in posts, comments and messages are off.
  image_upload_disabled         = $true
  image_allow_video_uploads     = $false
  image_mode                    = "none"
  nsfw_content_disallowed       = $false
  # English-only site: posts and comments can only be tagged English
  # (37) or left undetermined (0). The website itself is forced to English in audioprn/lemmy-ui.
  # The Content feed is sorted by New by default (spec).
  default_post_sort_type        = "new"
  discussion_languages          = @(0, 37)
}
Invoke-Api PUT "/site" $site | Out-Null
Write-Host "Site settings applied"

# --- Communities: main (the Content page) and creators (posts on creators' own pages) ---------
# The website shows only main on Content. Admins are the only moderators of creators.
$communities = @(
  @{ name = $CommunityName; title = $CommunityTitle; summary = "Links to audios and scripts from verified creators."; sidebar = "rules.md" },
  # Unlisted: not in the communities list (its posts show on each creator's profile).
  @{ name = "creators"; title = "Creator pages"; summary = "Posts on creators' own pages."; sidebar = "creator-pages.md"; visibility = "unlisted" }
)
foreach ($c in $communities) {
$CommunityName = $c.name
$community = $null
try { $community = (Invoke-Api GET "/community" @{ name = $CommunityName }).community_view } catch {}

$communitySettings = @{
  title   = $c.title
  summary = $c.summary
  sidebar = Read-Text $c.sidebar
  nsfw    = $true
  # Who can post is enforced by the backend (crates/audioprn): verified creators, moderators and admins.
  # So this upstream "mods only" switch stays off.
  posting_restricted_to_mods = $false
  # Posts get emoji reactions instead of votes (crates/audioprn). Comments keep up and down votes.
  post_upvotes   = "disable"
  post_downvotes = "disable"
  discussion_languages       = @(0, 37)
  visibility                 = $(if ($c.visibility) { $c.visibility } else { "public" })
}

if ($null -eq $community) {
  $community = (Invoke-Api POST "/community" ($communitySettings + @{ name = $CommunityName })).community_view
  Write-Host "Created community '$CommunityName'"
} else {
  Invoke-Api PUT "/community" ($communitySettings + @{ community_id = $community.community.id }) | Out-Null
  Write-Host "Updated community '$CommunityName'"
  $community = (Invoke-Api GET "/community" @{ name = $CommunityName }).community_view
}
$communityId = $community.community.id

# --- Post flairs (community tags) -------------------------------------------
# Every post needs exactly one of these (enforced in crates/audioprn). Tags not in this list are deleted.
$existing = @($community.tags | Where-Object { -not $_.deleted } | ForEach-Object { $_.name })
$tags = @(
  @{ name = "script_fill";  display_name = "Audio: Script Fill"; color = "color01" },
  @{ name = "script_offer"; display_name = "Script Offer";       color = "color02" },
  @{ name = "oc";           display_name = "Audio: OC";          color = "color03" },
  @{ name = "podcast_ama";  display_name = "Podcast/AMA";        color = "color04" },
  @{ name = "collection";   display_name = "Collection";         color = "color05" }
)
foreach ($tag in $tags) {
  if ($existing -contains $tag.name) { Write-Host "Tag '$($tag.name)' already exists"; continue }
  Invoke-Api POST "/community/tag" ($tag + @{ community_id = $communityId }) | Out-Null
  Write-Host "Created tag '$($tag.name)'"
}
foreach ($old in @($community.tags | Where-Object { -not $_.deleted -and $tags.name -notcontains $_.name })) {
  Invoke-Api DELETE "/community/tag" @{ tag_id = $old.id; delete = $true } | Out-Null
  Write-Host "Deleted tag '$($old.name)'"
}
}

Write-Host "`nPhase 1 settings done. Open $BaseUrl to check them."
