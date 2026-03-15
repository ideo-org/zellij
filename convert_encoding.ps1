$files = Get-ChildItem "N:\repos\zellij\zellij-server\src\panes\kitty_graphics\*.rs"
foreach ($f in $files) {
    $b = [System.IO.File]::ReadAllBytes($f.FullName)
    if ($b.Length -ge 2 -and $b[0] -eq 255 -and $b[1] -eq 254) {
        Write-Host ("Converting UTF-16LE to UTF-8: " + $f.Name)
        $content = [System.IO.File]::ReadAllText($f.FullName, [System.Text.Encoding]::Unicode)
        [System.IO.File]::WriteAllText($f.FullName, $content, (New-Object System.Text.UTF8Encoding $false))
        Write-Host ("Done: " + $f.Name)
    }
}
Write-Host "All done."
