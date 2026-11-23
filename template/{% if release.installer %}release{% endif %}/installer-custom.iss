; The project's own part of the Windows installer. installer.iss, which trellis rewrites on every
; update, includes this file before its own sections; trellis writes it once and never changes it
; after. Add what is particular to the program here, such as:
;
; - [Code]: Pascal Script, such as an uninstaller that waits while the program runs, which it can
;   tell by a mutex the program creates and CheckForMutexes reads, or one that asks whether to
;   delete the user's settings.
; - [InstallDelete]: the files an older version installed that this one doesn't, which an upgrade
;   would otherwise leave behind.
; - [UninstallDelete]: the files the program writes outside the files it was installed with, such
;   as downloaded updates.
; - [Run]: a program to start after the install, such as the app itself, with
;   `Flags: nowait postinstall skipifsilent`.
; - [Setup] directives installer.iss doesn't set, such as SetupIconFile, the installer's icon,
;   relative to this directory. Inno Setup refuses a directive set twice, so change the two
;   installer.iss lets you with a #define instead: `#define CloseApplications "force"` kills a
;   running process that has no window, which setup otherwise can't close, and
;   `#define SetupMutex "<name>"` keeps the mutex an earlier installer used.
;
; The script runs with these defines: Target, the target triple; AppVersion, the version;
; Staging, the folder of staged files; AppId, AppName and AppExe, the program's file name.
