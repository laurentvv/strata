# Strata 0.1.0

First public release of Strata, a fast disk space analyzer for Windows written in Rust. Scan a drive, see where
the space went in a live tree and a proportional map, find what can be cleaned, and keep a history to see what grew.

## What it does

- **Live scan**: the tree and the map fill in while the drive is read. Stop keeps what was found. Common folders,
  drives with their free space and recent folders are one click away on the start page; a folder can also be
  dropped onto the window, or chosen from Explorer's context menu (optional, in Options).
- **Proportional map** by file type or by folder level, **tree** with sortable columns, and views for file types,
  the 100 biggest files, the oldest files, file age, duplicate files, and a size-over-time chart.
- **Scan history**: every finished scan is stored; the Changes view ranks what grew or vanished since an earlier
  scan. `strata --cli PATH --changes` stores one without a window, so a scheduled task can feed it (see the README).
- **Clean-up**: reclaimable folders marked *Safe* or *Check first* (build output and dependency folders of your
  projects, caches, temporary files, crash dumps, `Windows.old`, empty folders, big files untouched for a year,
  duplicates). Dependency folders of installed programs are never offered.
- **Careful deletion**: Recycle Bin by default, after a confirmation; "Delete permanently" is an unticked option;
  drive roots, the Windows and program folders and the profile folders themselves are always refused. After a
  deletion, a banner shows the Recycle Bin size and offers to empty it (it asks first).
- **System space**: the Windows files that are easy to overlook (hibernation and paging files, restore points,
  the Recycle Bin, `WinSxS`, update downloads, the installer cache, `Windows.old`, the driver store) with their size,
  what they are, and Microsoft's own commands to reduce them. Strata deletes nothing there; you copy a command and
  run it as administrator.
- **Honest about what it cannot read**: without administrator rights, Strata says so, counts the folders Windows
  refused, and for a whole drive compares what the drive reports with what was found. One click restarts it as
  administrator.
- **Fast NTFS scan** (administrator, whole drive, opt-in): reads the Master File Table directly.
- **Opens without OpenGL**: when a machine has none (some virtual machines, remote sessions), the window is drawn
  with DirectX 12 instead, on the CPU if there is no graphics card. `--renderer opengl|directx|software` forces a mode.
- **Command line**: headless tree, export to CSV, JSON or HTML, changes since the last scan, clean-up list.
- English and French, light and dark theme, keyboard shortcuts (listed in the README).

## Download

| File | What it is |
|---|---|
| `strata-v0.1.0-windows-x64.zip` | `strata.exe` alone: unpack anywhere and run. Create an empty `strata.portable` next to it to keep settings and history beside the program (a `strata.portable.example` is included). |
| `strata-v0.1.0-windows-x64.msi` | Per-user installer: no administrator rights, installs in `%LOCALAPPDATA%\Programs\Strata`, Start menu shortcut, clean uninstall. |
| `*.sha256` | SHA-256 of each file. |

To check a download in PowerShell (prints `True` when it matches):

```powershell
(Get-FileHash .\strata-v0.1.0-windows-x64.zip -Algorithm SHA256).Hash.ToLower() -eq (Get-Content .\strata-v0.1.0-windows-x64.zip.sha256).Split(' ')[0]
```

**The program is not code-signed yet**, so Windows SmartScreen may show "unknown publisher" the first time you run
it or the installer. It is built on GitHub's runners from the tagged source; the build fails if the executable
contains build-machine paths or needs the Visual C++ runtime, and the installer is installed, run and uninstalled
on the runner before the files are published.

## Measured

- `C:\Windows`: about 3 to 8 s to scan; about 230 MB of memory at the peak and 160 MB once done.
- A drive with 3.75 million files: about 12 s on a fast SSD, about 1 GB of memory at the peak.
- NTFS fast scan, checked on a 252 GB volume with 95,078 files: same sizes and folder count as the normal scan, every
  hard link listed under its own name, 3 more files (in `System Volume Information`, which the normal scan may not
  read), in 1.2 s instead of 79 s on a cold cache.
- Files: `strata.exe` about 15 MB, zip about 6 MB, MSI about 5 MB.

## Known limits

- Tested on Windows 11 with NTFS drives, and in a Hyper-V virtual machine. Windows 10, ARM, network shares and
  OneDrive "online-only" files have not been tried.
- Sizes count every name of a file that has several (hard links), so `WinSxS` and the total of a system drive can
  look bigger than the space really used; the System space view says so.
- The software DirectX path uses a little more memory and some CPU than OpenGL.
- The commands shown in System space are Microsoft's documented ones, but Strata does not run them: read the text
  next to each before using them.
- Strata never updates itself. The optional update check (off by default: Options > Updates, or
  `strata --check-update`) only tells you when a newer release exists and opens its page. It asks GitHub at most once a
  day; GitHub sees your IP address and nothing about your files.

## Reporting a problem

Open an issue at <https://github.com/laurentvv/strata/issues>. In the app, **Help > About > Copy diagnostics**
gives the version, Windows build and graphics driver to paste; if Strata crashed or did not start, `crash.log` in
`%APPDATA%\strata` (or `strata-data` next to a portable `strata.exe`) helps. Please read it first and keep anything
private out of the report.

MIT license.
