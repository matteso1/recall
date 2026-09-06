# Recall auto-start: keep the overlay alive exactly while the League client is running.
# Installed into the Startup folder by scripts/autostart-install.sh (which also copies this file
# to %LOCALAPPDATA%\Recall). Runs hidden at logon; polls every few seconds; needs no admin rights.
#
# - League client up and no overlay: start the canonical recall.exe.
# - League client gone for a while and the overlay was started by us: stop it.
# - The overlay quit by hand (the x button) is not restarted until the client restarts.
param(
    [string]$Exe = "$env:USERPROFILE\code\recall-win\overlay\target\swiftplay\release\recall.exe",
    [int]$PollSeconds = 5,
    [int]$CloseAfterSeconds = 20
)

$log = Join-Path $env:LOCALAPPDATA "Recall\autostart.log"
New-Item -ItemType Directory -Force -Path (Split-Path $log) | Out-Null
function Log($msg) { Add-Content -Path $log -Value ("{0:yyyy-MM-ddTHH:mm:ssK} {1}" -f (Get-Date), $msg) }

# One watcher at a time: a second copy (a second logon shortcut, a manual start) exits.
$mutex = New-Object System.Threading.Mutex($false, "Local\RecallAutostart")
if (-not $mutex.WaitOne(0)) { exit 0 }

Log "watching for LeagueClientUx.exe; overlay $Exe"
$startedByUs = $false
$clientMissingSince = $null
$sawClientSinceQuit = $true

while ($true) {
    $client = Get-Process -Name LeagueClientUx -ErrorAction SilentlyContinue
    $overlay = Get-Process -Name recall -ErrorAction SilentlyContinue
    if ($client) {
        $clientMissingSince = $null
        if (-not $overlay) {
            if ($sawClientSinceQuit) {
                if (Test-Path $Exe) {
                    Start-Process -FilePath $Exe -WorkingDirectory (Split-Path $Exe)
                    $startedByUs = $true
                    $sawClientSinceQuit = $false
                    Log "client up, overlay started"
                    Start-Sleep -Seconds $PollSeconds
                    continue
                } else {
                    Log "client up but the overlay is not built: $Exe"
                    Start-Sleep -Seconds 60
                }
            }
        } elseif (-not $startedByUs) {
            # Someone launched it by hand while the client is up: adopt it so we close it later.
            $startedByUs = $true
        }
    } else {
        # The client is gone: after a grace period close an overlay we started (patch restarts
        # bring the client back within seconds and must not bounce the panel).
        $sawClientSinceQuit = $true
        if ($overlay -and $startedByUs) {
            if (-not $clientMissingSince) { $clientMissingSince = Get-Date }
            if (((Get-Date) - $clientMissingSince).TotalSeconds -ge $CloseAfterSeconds) {
                Stop-Process -Name recall -ErrorAction SilentlyContinue
                $startedByUs = $false
                $clientMissingSince = $null
                Log "client closed, overlay stopped"
            }
        }
    }
    # The overlay was closed by hand while the client is up: leave it closed until the client
    # restarts (the x button means "not this game").
    if ($client -and -not $overlay -and $startedByUs) {
        $startedByUs = $false
        $sawClientSinceQuit = $false
        Log "overlay quit by hand; not restarting until the client restarts"
    }
    Start-Sleep -Seconds $PollSeconds
}
