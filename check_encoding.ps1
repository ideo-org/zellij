$files = Get-ChildItem "N:\repos\zellij\zellij-server\src\panes\kitty_graphics\*.rs"
foreach ($f in $files) {
    $b = [System.IO.File]::ReadAllBytes($f.FullName)
    if ($b.Length -ge 2 -and $b[0] -eq 255 -and $b[1] -eq 254) {
        Write-Host ("UTF-16LE: " + $f.Name)
    } elseif ($b.Length -ge 3 -and $b[0] -eq 239 -and $b[1] -eq 187 -and $b[2] -eq 191) {
        Write-Host ("UTF-8-BOM: " + $f.Name)
    } else {
        Write-Host ("UTF-8: " + $f.Name)
    }
}
