<#
  audioprn: turns a guide's Word file (.docx) into markdown for the guide pages
  (lemmy-ui/src/assets/guides/<name>.md, shown at /guides/<name>).
  Keeps headings (Title / Heading styles), lists (with indents), bold, italic, web links,
  links inside the document (they jump to the headings), and tables.

  Usage (from audioprn/setup):
    .\guide-from-docx.ps1 "C:\path\Tagging guide.docx" ..\..\..\lemmy-ui\src\assets\guides\tagging.md
  Then rebuild the website.
#>
param(
  [Parameter(Mandatory)] [string]$Docx,
  [Parameter(Mandatory)] [string]$Out
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.IO.Compression.FileSystem

$zip = [IO.Compression.ZipFile]::OpenRead((Resolve-Path $Docx).Path)
function Read-Entry($name) {
  $reader = New-Object IO.StreamReader($zip.GetEntry($name).Open())
  try { $reader.ReadToEnd() } finally { $reader.Close() }
}
[xml]$doc = Read-Entry "word/document.xml"
[xml]$rels = Read-Entry "word/_rels/document.xml.rels"
$zip.Dispose()

$W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
$RelNs = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"  # not $R: PowerShell names ignore case ($r is a run below)
$ns = New-Object Xml.XmlNamespaceManager $doc.NameTable
$ns.AddNamespace("w", $W)
$links = @{}
foreach ($rel in $rels.Relationships.Relationship) { $links[$rel.Id] = $rel.Target }

# Same as slug() in lemmy-ui/src/shared/audioprn/guides.tsx, so links match heading ids.
function Slug($text) {
  $s = ($text.ToLower() -replace '[^a-z0-9]+', '-').Trim('-')
  if ($s) { $s } else { "section" }
}
function Plain($node) { (($node.SelectNodes(".//w:t", $ns) | ForEach-Object { $_.InnerText }) -join "").Trim() }
# Bookmarks that links inside the document point to.
$targets = @{}
foreach ($h in $doc.SelectNodes("//w:hyperlink[@w:anchor]", $ns)) { $targets[$h.GetAttribute("anchor", $W)] = $true }
# Headings: Title / Heading styles, and short paragraphs that a link points to.
function Is-Heading($p) {
  if ($p.SelectSingleNode("w:pPr/w:pStyle/@w:val", $ns).Value -match '^(Title|Heading\d)$') { return $true }
  $text = Plain $p
  $text -and $text.Length -le 80 -and ($p.SelectNodes(".//w:bookmarkStart", $ns) | Where-Object { $targets[$_.GetAttribute("name", $W)] })
}

# Where links inside the document jump to: bookmarks in headings use the heading's id; other
# linked bookmarks get an invisible anchor where they are (see Marker). Links to a missing
# bookmark fall back to the heading with the link's text (e.g. "A" -> the "A" heading).
$anchors = @{}
$headingSlugs = @{}
foreach ($p in $doc.SelectNodes("//w:body/w:p", $ns)) {
  if (Is-Heading $p) {
    $slug = Slug (Plain $p)
    $headingSlugs[$slug] = $true
    foreach ($b in $p.SelectNodes(".//w:bookmarkStart", $ns)) { $anchors[$b.GetAttribute("name", $W)] = "#$slug" }
  }
}
function Bookmark-Id($name) { "bm-" + ($name -replace '[^A-Za-z0-9_-]', '-') }
foreach ($name in @($targets.Keys)) {
  if (-not $anchors[$name] -and $doc.SelectSingleNode("//w:bookmarkStart[@w:name='$name']", $ns)) {
    $anchors[$name] = "#" + (Bookmark-Id $name)
  }
}
# An invisible anchor for a linked bookmark: an empty link that guides.tsx turns into an id.
function Marker($name) {
  if ($targets[$name] -and $anchors[$name] -like "#bm-*") { "[](#!$(Bookmark-Id $name))" } else { "" }
}

# Markdown characters in the text. Square brackets stay as they are, for tags like [F4M].
function Escape($text) {
  $t = $text -replace '([\\`*~])', '\$1' -replace '(?<!\w)_|_(?!\w)', '\_'
  $t -replace '^(\s*)(#|>|[-+] |\d+\. )', '$1\$2'
}
function Is-On($rPr, $tag) {
  $e = $rPr.SelectSingleNode("w:$tag", $ns)
  $e -and $e.GetAttribute("val", $W) -notin @("0", "false")
}
# A paragraph's (or table cell's) text with bold, italic and links.
function Inline($node) {
  $segs = New-Object Collections.ArrayList
  foreach ($r in $node.SelectNodes(".//w:r", $ns)) {
    $text = ""
    foreach ($c in $r.ChildNodes) {
      if ($c.LocalName -eq "t") { $text += $c.InnerText } elseif ($c.LocalName -in @("tab", "br")) { $text += " " }
    }
    if (-not $text) { continue }
    $href = $null
    $h = $r.ParentNode
    if ($h.LocalName -eq "hyperlink") {
      $id = $h.GetAttribute("id", $RelNs)
      $anchor = $h.GetAttribute("anchor", $W)
      if ($id -and $links[$id]) { $href = $links[$id] -replace ' ', '%20' -replace '\(', '%28' -replace '\)', '%29' }
      elseif ($anchor -and $anchors[$anchor]) { $href = $anchors[$anchor] }
      elseif ($anchor -and $headingSlugs[(Slug (Plain $h))]) { $href = "#" + (Slug (Plain $h)) }
    }
    $rPr = $r.SelectSingleNode("w:rPr", $ns)
    $bold = $rPr -and (Is-On $rPr "b")
    $italic = $rPr -and (Is-On $rPr "i")
    $last = if ($segs.Count) { $segs[$segs.Count - 1] } else { $null }
    if ($last -and $last.bold -eq $bold -and $last.italic -eq $italic -and $last.href -eq $href) {
      $last.text += $text
    } else {
      [void]$segs.Add([pscustomobject]@{ text = $text; bold = $bold; italic = $italic; href = $href })
    }
  }
  ($segs | ForEach-Object {
    $t = Escape $_.text
    # Markers go around the words, spaces stay outside: " a " -> " **a** ".
    if ($t.Trim() -and ($_.bold -or $_.italic)) {
      $m = [regex]::Match($t, '^(\s*)(.*?)(\s*)$')
      $mark = if ($_.bold -and $_.italic) { "***" } elseif ($_.bold) { "**" } else { "*" }
      $t = $m.Groups[1].Value + $mark + $m.Groups[2].Value + $mark + $m.Groups[3].Value
    }
    if ($_.href -and $t.Trim()) { $t = "[$t]($($_.href))" }
    $t
  }) -join ""
}

$lines = New-Object Collections.ArrayList
$inList = $false
$markers = ""  # anchors waiting for the next paragraph with text
foreach ($node in $doc.SelectSingleNode("//w:body", $ns).ChildNodes) {
  if ($node.LocalName -eq "bookmarkStart") { $markers += Marker $node.GetAttribute("name", $W); continue }
  if ($node.LocalName -eq "p") {
    $markers += ($node.SelectNodes(".//w:bookmarkStart", $ns) | ForEach-Object { Marker $_.GetAttribute("name", $W) }) -join ""
  }
  if ($markers -and ($node.LocalName -eq "tbl" -or (Is-Heading $node))) {
    [void]$lines.Add(""); [void]$lines.Add($markers); $markers = ""
  }
  if ($node.LocalName -eq "tbl") {
    $rows = $node.SelectNodes("w:tr", $ns) | ForEach-Object {
      "| " + (($_.SelectNodes("w:tc", $ns) | ForEach-Object { ((Inline $_) -replace '\|', '\|').Trim() }) -join " | ") + " |"
    }
    if ($rows) {
      $cols = ([regex]::Matches($rows[0], ' \| ').Count) + 1
      [void]$lines.Add(""); [void]$lines.Add($rows[0])
      [void]$lines.Add("|" + ((1..$cols | ForEach-Object { " --- " }) -join "|") + "|")
      $rows | Select-Object -Skip 1 | ForEach-Object { [void]$lines.Add($_) }
      [void]$lines.Add("")
    }
    $inList = $false
    continue
  }
  if ($node.LocalName -ne "p") { continue }
  if (Is-Heading $node) {
    $text = Plain $node
    if ($text) { [void]$lines.Add(""); [void]$lines.Add("## " + $text); [void]$lines.Add("") }
    $inList = $false
    continue
  }
  $text = (Inline $node).Trim()
  if (-not $text) { continue }
  $text = $markers + $text
  $markers = ""
  $level = $node.SelectSingleNode("w:pPr/w:numPr/w:ilvl/@w:val", $ns)
  if ($level) {
    if (-not $inList) { [void]$lines.Add("") }
    [void]$lines.Add(("  " * [int]$level.Value) + "- " + $text)
    $inList = $true
  } else {
    [void]$lines.Add(""); [void]$lines.Add($text)
    $inList = $false
  }
}

$md = (($lines -join "`n") -replace "`n{3,}", "`n`n").Trim() + "`n"
New-Item -ItemType Directory -Force (Split-Path $Out) | Out-Null
[IO.File]::WriteAllText((Join-Path (Resolve-Path (Split-Path $Out)).Path (Split-Path $Out -Leaf)), $md, (New-Object Text.UTF8Encoding $false))
"Wrote $Out ($([math]::Round($md.Length / 1KB)) KB, $(($lines | Where-Object { $_ -like '## *' }).Count) headings)"
