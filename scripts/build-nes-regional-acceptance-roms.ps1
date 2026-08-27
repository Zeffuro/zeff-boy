param(
    [string]$OutDir = "rom-tests/cache/nes/regional-acceptance"
)

$ErrorActionPreference = "Stop"

$commit = "95d8f621ae55cee0d09b91519a8989ae0e64753b"
$baseUrl = "https://raw.githubusercontent.com/christopherpow/nes-test-roms/$commit"

$roms = @(
    @{ Path = "pal_apu_tests/01.len_ctr.nes"; Output = "pal-01.len_ctr.nes"; Source = "5E4A07738703232DFEFCE6A26F12DA304F333008C60224B27E7FBADF4A7CDC0C"; Derived = "567CDC921E31CCA681C75F1748D01359B4797A0DF3382AE8A8C75978597829C0"; Timing = "pal" },
    @{ Path = "pal_apu_tests/02.len_table.nes"; Output = "pal-02.len_table.nes"; Source = "AC5537885469A85E733DF1A7A6A0A76A76F157F080C60D04F1128902A45423D4"; Derived = "432CEB520AB08E9A33017CDA46BA960BD0EB1E582FC64C16BA7B10F54DA903FF"; Timing = "pal" },
    @{ Path = "pal_apu_tests/03.irq_flag.nes"; Output = "pal-03.irq_flag.nes"; Source = "E0C04111C61D0FC671990C5C3AC6CB7F57082AD687B5E11D380277C7D75E56D1"; Derived = "012DFCC6E562D58A2F775223646B07A9C7117B6E5EA90A4C4C261465987631BC"; Timing = "pal" },
    @{ Path = "pal_apu_tests/04.clock_jitter.nes"; Output = "pal-04.clock_jitter.nes"; Source = "DC85B14F7ECE5E7BD4010B831F5B796DEBFDF338837C8A29A1D221DE8C63776D"; Derived = "2F56C7FFFE824F3DC75F6BBE766F4607838234DF443EFBEBFFAA50B3778F5A72"; Timing = "pal" },
    @{ Path = "pal_apu_tests/05.len_timing_mode0.nes"; Output = "pal-05.len_timing_mode0.nes"; Source = "04896F081373F5AB6CE83CE115C5FC0FF823ACF831F1499D7D406F4A651E7CBC"; Derived = "C317EE702662E60E25FE44676DD0842F74D9EE01B94C6DAB5B4A6816A5ECE583"; Timing = "pal" },
    @{ Path = "pal_apu_tests/06.len_timing_mode1.nes"; Output = "pal-06.len_timing_mode1.nes"; Source = "454B1B6339BD2EA27E3F4E8A8DE7E2D95E3AFC26940A88255E24A033D42D5A05"; Derived = "30AD8F77BA4654100348BFE607CA78492226B0E0577646E4FD8F723771F3490E"; Timing = "pal" },
    @{ Path = "pal_apu_tests/07.irq_flag_timing.nes"; Output = "pal-07.irq_flag_timing.nes"; Source = "C91AA1FC7BCB2638F3B07996270EB38C67E8B0FEFA1A0DB02A34B2E2FFD883C7"; Derived = "9F7FA42A25CE44E73F660F9DCDCACECADA7A49B6BD40D47CDDA45F9F2DB703A5"; Timing = "pal" },
    @{ Path = "pal_apu_tests/08.irq_timing.nes"; Output = "pal-08.irq_timing.nes"; Source = "DEE9E8FAC623327B04E8160456362CC1FE4CA0B2C8E3F45EEDCB6851EBB00AAE"; Derived = "F555B7D3B7C45C6828F836C686E454703CF8218D6C339A7C7752DA18531DCB00"; Timing = "pal" },
    @{ Path = "pal_apu_tests/10.len_halt_timing.nes"; Output = "pal-10.len_halt_timing.nes"; Source = "C41238ED0E7F4044C21FCD14C99B9E4516611ADBEE5C5F139D3BB95BEBEBCEC9"; Derived = "BCAECA140BF720B7B2A0A7E99F2EB925F2C1837F933FBA5018D0B8180A662889"; Timing = "pal" },
    @{ Path = "pal_apu_tests/11.len_reload_timing.nes"; Output = "pal-11.len_reload_timing.nes"; Source = "1E94A9C0D829378F93B460C2C5F875418490401AFD50C30CD05EA22113819909"; Derived = "334558AB8EEA1231419B14AAE74D23505A5D07B4BA787F54F83BC114A50B6043"; Timing = "pal" },
    @{ Path = "nmi_sync/demo_pal.nes"; Output = "pal-demo_pal.nes"; Source = "8848FB7F7A20C9ACB58A4CDEAE2D04A9FD4A33159524CEC1F77C802D36861851"; Derived = "D5C9698C5687903FD8358718FFCB0101BF9024CDF296CF78EEA7BB13D7AAD98B"; Timing = "pal" },
    @{ Path = "240pee/240pee.nes"; Output = "dendy-240pee.nes"; Source = "228A370B32DAACEC4C95927AA18243A57BE2D45D1D038479BA9D4BB19D05985E"; Derived = "CF76F0554497E130AC1ACE629529A8E94BF7E686F8DDE4255FBD0A334D494D68"; Timing = "dendy" }
)

New-Item -ItemType Directory -Path $OutDir -Force | Out-Null

foreach ($rom in $roms) {
    $outputPath = Join-Path $OutDir $rom.Output
    Invoke-WebRequest -Uri "$baseUrl/$($rom.Path)" -OutFile $outputPath
    $sourceHash = (Get-FileHash -LiteralPath $outputPath -Algorithm SHA256).Hash
    if ($sourceHash -ne $rom.Source) {
        throw "unexpected source hash for $($rom.Path): $sourceHash"
    }

    $bytes = [IO.File]::ReadAllBytes($outputPath)
    if ($rom.Timing -eq "pal") {
        $bytes[9] = $bytes[9] -bor 0x01
    } else {
        $bytes[7] = ($bytes[7] -band 0xF3) -bor 0x08
        $bytes[12] = ($bytes[12] -band 0xFC) -bor 0x03
    }
    [IO.File]::WriteAllBytes($outputPath, $bytes)

    $derivedHash = (Get-FileHash -LiteralPath $outputPath -Algorithm SHA256).Hash
    if ($derivedHash -ne $rom.Derived) {
        throw "unexpected derived hash for $($rom.Output): $derivedHash"
    }
    Write-Output "$($rom.Output) $derivedHash"
}
