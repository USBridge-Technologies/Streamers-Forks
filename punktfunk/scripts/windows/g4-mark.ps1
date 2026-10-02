<#
.SYNOPSIS
  Append one EVENT marker to a G4 soak log.
.DESCRIPTION
  The payload of the wake-to-run task in g4-soak.ps1's sleep leg: its line in live-g4-<Tag>.log
  is the proof the box woke itself rather than being woken by a person.
#>
param([string]$Tag = 'g4n', [string]$Kind = 'wake')
"$((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ss'))Z EVENT kind=$Kind source=task" |
  Add-Content "C:\Users\Public\live-g4-$Tag.log"
