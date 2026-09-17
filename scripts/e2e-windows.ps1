<#
.SYNOPSIS
  End-to-end check of a PUBLISHED agent-sync release on a real Windows machine.

.DESCRIPTION
  Step 3 of the runbook's Verifying section, made runnable.

  release.yml's build job does smoke-run `--version` on the freshly built
  binary, on a matching runner, for every target except aarch64-pc-windows-msvc
  (cross-compiled, run_check=false, executed nowhere). What nothing else does is
  run the PUBLISHED zip - after packaging, upload and checksumming - or exercise
  any behaviour beyond `--version`. All three bugs 1.0.1 fixed are invisible to
  a version check: symlink flavour, revert's delete, and the import line.

  It fetches the release asset anonymously, checks it against SHA256SUMS.txt,
  and drives the whole journey: init, doctor, sync, mcp, status, adopt, revert.

  Three of the assertions are the Windows bugs fixed in 1.0.1 and are marked as
  such - directory-symlink flavour on create, directory-symlink deletion on
  revert (1.0.0 failed with OS error 5), and forward slashes in the `~` import
  line (1.0.0 wrote `@~/.agents\AGENTS.md`).

.PARAMETER Tag
  The release tag to verify, e.g. agent-sync-v1.0.1. The version and asset name
  are derived from it.

.NOTES
  Requires:
    * a real Windows box - this is exactly what a CI runner does not prove;
    * Developer Mode OR an elevated shell, or every symlink fails with OS
      error 1314 and the run is meaningless rather than merely red;
    * outbound HTTPS to github.com. No toolchain, no npm, no checkout of the
      crate - it tests the shipped binary, not the source.

  Isolation: everything happens under a scratch HOME in $env:TEMP. The script
  refuses to start if that redirection is not honoured, and fingerprints the
  real profile's ~/.claude and ~/.agents before and after, failing loudly if
  either moved. A fan-out that escaped into a real Commons is the one failure
  mode here that would damage the machine it is testing.

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File scripts\e2e-windows.ps1 -Tag agent-sync-v1.0.1
#>
param(
  [Parameter(Mandatory = $true)]
  [ValidatePattern('^agent-sync-v\d+\.\d+\.\d+$')]
  [string]$Tag
)

$ErrorActionPreference = "Continue"
$OutputEncoding = [Console]::OutputEncoding = [Text.UTF8Encoding]::new()

$version = $Tag -replace '^agent-sync-v', ''
$arch = if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") { "arm64" } else { "x64" }
$asset = "agent-sync-$version-win32-$arch.zip"
$base = "https://github.com/agent-sync-sh/agent-sync/releases/download/$Tag"

$script:pass = 0; $script:fail = 0
function Check($name, $cond, $detail) {
  if ($cond) { $script:pass++; "  PASS  $name" }
  else { $script:fail++; "  FAIL  $name" + $(if ($detail) { "  -- $detail" } else { "" }) }
}

# A cheap content fingerprint: relative path, size and mtime of every file.
function Fingerprint($path) {
  if (-not (Test-Path $path)) { return "<absent>" }
  (Get-ChildItem -Recurse -Force -File $path -ErrorAction SilentlyContinue |
    Sort-Object FullName |
    ForEach-Object { "$($_.FullName)|$($_.Length)|$($_.LastWriteTimeUtc.Ticks)" }) -join "`n"
}

$realProfile = $env:USERPROFILE
$realClaude = Join-Path $realProfile ".claude"
$realCommons = Join-Path $realProfile ".agents"
$beforeClaude = Fingerprint $realClaude
$beforeCommons = Fingerprint $realCommons

"agent-sync e2e - $Tag ($asset) on $env:COMPUTERNAME"
"real profile guarded: $realClaude, $realCommons"
""

$root = Join-Path $env:TEMP "as-e2e"
Remove-Item -Recurse -Force $root -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $root | Out-Null

"=== 0. fetch and verify the published artifact ==="
$zip = Join-Path $root $asset
Invoke-WebRequest -UseBasicParsing -OutFile $zip "$base/$asset"
# Downloaded to a file rather than read from .Content: GitHub serves
# SHA256SUMS.txt as application/octet-stream, and Windows PowerShell 5.1 hands
# back a byte[] for a non-text content type, which -split silently turns into
# nothing at all.
$sumsFile = Join-Path $root "SHA256SUMS.txt"
Invoke-WebRequest -UseBasicParsing -OutFile $sumsFile "$base/SHA256SUMS.txt"
$line = Select-String -Path $sumsFile -SimpleMatch $asset | Select-Object -First 1
$want = if ($line) { ($line.Line.Trim() -split '\s+')[0].ToLower() } else { $null }
$got = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
Check "checksum matches SHA256SUMS.txt" ($want -and $got -eq $want) "want=$want got=$got"
Expand-Archive $zip -DestinationPath $root -Force
$exe = Join-Path $root "agent-sync.exe"
Check "the archive contains agent-sync.exe" (Test-Path $exe)
if (-not (Test-Path $exe)) { "aborting: nothing to test"; exit 1 }

# --- scratch HOME ------------------------------------------------------------
$h = Join-Path $root "home"
New-Item -ItemType Directory -Path $h | Out-Null
$env:HOME = $h; $env:USERPROFILE = $h
Set-Location $h
function Run() { & $exe @args 2>&1 | Out-String }

"=== 0b. the scratch HOME is honoured (refuse to touch the real profile) ==="
$o = Run init
Check "init created the scratch Commons" (Test-Path "$h\.agents\skills") $o.Trim()
Check "the real Commons was not initialised into" `
  ((Fingerprint $realCommons) -eq $beforeCommons) "the binary ignored USERPROFILE"
if ((Fingerprint $realCommons) -ne $beforeCommons) {
  "ABORTING: the scratch HOME is not being honoured; refusing to run a fan-out"
  exit 1
}

"=== 1. version ==="
$o = Run --version
Check "reports $version" ($o -match ([regex]::Escape("agent-sync $version"))) $o.Trim()

"=== 2. doctor on a bare machine names the fix ==="
$bare = Join-Path $root "bare"
New-Item -ItemType Directory -Path $bare | Out-Null
$env:HOME = $bare; $env:USERPROFILE = $bare
$o = Run doctor
Check "bare machine tells you to init" ($o -match "agent-sync init") $o.Trim()
$env:HOME = $h; $env:USERPROFILE = $h

"=== 3. init is idempotent ==="
$o = Run init
Check "second init says already present" ($o -match "already present") $o.Trim()
Check "init made no agent roots" (-not (Test-Path "$h\.claude"))

"=== 4. populate the Commons ==="
New-Item -ItemType Directory -Path "$h\.agents\skills\research" -Force | Out-Null
Set-Content "$h\.agents\skills\research\SKILL.md" "# research`nthe skill body"
Set-Content "$h\.agents\AGENTS.md" "shared instructions for every agent"
Set-Content "$h\.agents\mcp.json" '{"mcpServers":{"fetch":{"command":"uvx","args":["mcp-server-fetch"]}}}'
New-Item -ItemType Directory -Path "$h\.claude", "$h\.codex", "$h\.gemini" -Force | Out-Null
Check "agent roots exist" ((Test-Path "$h\.claude") -and (Test-Path "$h\.codex"))

"=== 5. doctor detects the installed agents ==="
$o = Run doctor
Check "doctor sees claude" ($o -match "claude")
Check "doctor sees codex" ($o -match "codex")
Check "doctor counts the skill" ($o -match "skills\s+1")
Check "doctor exits clean" ($LASTEXITCODE -eq 0) "exit $LASTEXITCODE"

"=== 6. sync fans out ==="
$o = Run sync
Check "sync succeeds" ($LASTEXITCODE -eq 0) $o.Trim()
$link = Get-Item "$h\.claude\skills\research" -Force -ErrorAction SilentlyContinue
Check "skill fanned out to claude" ($null -ne $link)
Check "it is a symlink" ($link -and $link.LinkType -eq "SymbolicLink") "LinkType=$($link.LinkType)"
Check "DIRECTORY flavour [1.0.1 fix]" `
  ($link -and ($link.Attributes -band [IO.FileAttributes]::Directory)) "attrs=$($link.Attributes)"
Check "content readable through it" `
  ((Get-Content "$h\.claude\skills\research\SKILL.md" -Raw) -match "the skill body")

"=== 7. the import line uses forward slashes [1.0.1 fix] ==="
$imp = Select-String -Path "$h\.claude\CLAUDE.md" -Pattern "@~" -ErrorAction SilentlyContinue
Check "import line present" ($null -ne $imp)
Check "no backslash in the ~ reference" ($imp -and $imp.Line -notmatch '\\') $(if ($imp) { $imp.Line.Trim() })
Check "points at the Commons AGENTS.md" `
  ($imp -and $imp.Line -match "~/\.agents/AGENTS\.md") $(if ($imp) { $imp.Line.Trim() })

"=== 8. MCP is visible to the agents ==="
$o = Run mcp list
Check "mcp list names the server" ($o -match "fetch") $o.Trim()

"=== 9. status is clean right after sync ==="
$o = Run status
Check "status exits clean" ($LASTEXITCODE -eq 0) "exit $LASTEXITCODE"

"=== 10. sync is idempotent ==="
$o = Run sync
Check "second sync makes no changes" ($o -notmatch "\b[1-9]\d* changes? made") $o.Trim()

"=== 11. adopt a repo path becomes a Sourced entry ==="
New-Item -ItemType Directory -Path "$h\repo\.git" -Force | Out-Null
Set-Content "$h\repo\.git\HEAD" "ref: refs/heads/main"
New-Item -ItemType Directory -Path "$h\repo\skills\deploy" -Force | Out-Null
Set-Content "$h\repo\skills\deploy\SKILL.md" "# deploy`nkept in the repo"
$o = Run adopt "$h\repo\skills\deploy"
Check "adopt succeeds" ($LASTEXITCODE -eq 0) $o.Trim()
$cl = Get-Item "$h\.agents\skills\deploy" -Force -ErrorAction SilentlyContinue
Check "commons links out to the repo" ($cl -and $cl.LinkType -eq "SymbolicLink") "LinkType=$($cl.LinkType)"
Check "reads through the source link" `
  ((Get-Content "$h\.agents\skills\deploy\SKILL.md" -Raw) -match "kept in the repo")

"=== 12. doctor lists the Sourced entry ==="
$o = Run doctor
Check "doctor marks it sourced" ($o -match "sourced") $o.Trim()
Check "doctor names the source path" ($o -match "repo")

"=== 13. the adopted skill reaches the agents ==="
$o = Run sync
Check "adopted skill fanned out" (Test-Path "$h\.claude\skills\deploy\SKILL.md")

"=== 14. revert refuses while the target is enabled ==="
$o = Run revert claude
Check "revert refuses an enabled target" ($LASTEXITCODE -ne 0 -and $o -match "still enabled") $o.Trim()

"=== 15. revert removes everything after disabling ==="
$cfg = "$h\.config\agent-sync\agent-sync.toml"
New-Item -ItemType Directory -Path (Split-Path $cfg) -Force | Out-Null
Add-Content $cfg "`n[targets]`nclaude = false`n"
$o = Run revert claude
Check "revert succeeds" ($LASTEXITCODE -eq 0) $o.Trim()
Check "directory symlink deleted [1.0.1 fix]" (-not (Test-Path "$h\.claude\skills\research"))
Check "adopted link deleted" (-not (Test-Path "$h\.claude\skills\deploy"))
$imp2 = Select-String -Path "$h\.claude\CLAUDE.md" -Pattern "@~" -ErrorAction SilentlyContinue
Check "import line removed" ($null -eq $imp2) $(if ($imp2) { $imp2.Line.Trim() })
Check "other agents untouched" (Test-Path "$h\.codex")

"=== 16. the Commons itself survived revert ==="
Check "commons skill intact" (Test-Path "$h\.agents\skills\research\SKILL.md")
Check "repo source intact" (Test-Path "$h\repo\skills\deploy\SKILL.md")

# --- the guard, checked last -------------------------------------------------
$env:USERPROFILE = $realProfile; $env:HOME = $realProfile
Set-Location $env:TEMP
"=== 17. the real profile was never touched ==="
Check "real ~/.claude unchanged" ((Fingerprint $realClaude) -eq $beforeClaude)
Check "real ~/.agents unchanged" ((Fingerprint $realCommons) -eq $beforeCommons)

""
"=================================================="
"e2e on real Windows - shipped $Tag binary"
"  PASSED: $script:pass"
"  FAILED: $script:fail"
"=================================================="
if ($script:fail -gt 0) { exit 1 } else { exit 0 }
