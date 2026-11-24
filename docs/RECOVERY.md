# If Tessera can't log you in

Tessera aims to never stop your computer from starting. At worst you can still log in with your password and fix things. At best nothing goes wrong at all. This page says what protects the login, and what to do by hand if all of that fails.

## Getting in from a console

1. Press **Ctrl+Alt+F2**. A text console opens; it doesn't depend on any login screen.
2. Log in with your user name and password.
3. Run:

   ```
   sudo /usr/lib/tessera/tessera-root rescue
   ```

   This takes Tessera's lines out of the login screen's PAM files (your fingerprint and keyring unlocking; a passkey line an older Tessera wrote, too), keeping a copy of each file in `/var/lib/tessera/rescue/`. It switches greetd to its own text login (`agreety`) in place of Tessera's login screen, then starts greetd again.
4. Press **Ctrl+Alt+F1**. Log in there, with your password, and your desktop starts as usual.
5. Once you're in, Tessera shows what was taken out, with a **Restore** button. Restore puts things back only once they would no longer stop the login screen. From a terminal, the same is `pkexec /usr/lib/tessera/tessera-root rescue undo`. `sudo /usr/lib/tessera/tessera-root rescue status` shows the note.

If `/usr/lib/tessera/tessera-root` is missing, you can do the same by hand:

```
sudo sed -i '/# Tessera /d; /# tessera: unlock the keyring/,+1d' /etc/pam.d/greetd
sudo systemctl reset-failed greetd && sudo systemctl restart greetd
```

To start your desktop from the console without any login screen, run `start-hyprland`.

## What protects the login

**What Tessera writes is safe by itself.**
- Every PAM line Tessera adds starts with `-`. Each one is `sufficient` or `optional`, or it's a gate that skips one line. A module that's missing (fprintd, pam-u2f or Howdy removed later) is skipped, and the password follows as before.
- Before any PAM file is written, the root helper checks the new text:
  - A login screen's file must still start a session with no password first. Tessera checks this by running the real Linux-PAM's `pam_setcred` on a copy. That's how greetd starts its greeter, and it's what broke on 22 November.
  - In every file, the password's own lines must still be reached in order whatever Tessera's lines answer. Tessera works this out by reading the stack, following its includes. It never runs it, so nothing asks for a finger or a password, and no failed attempt is counted.
- The file from before each edit is kept in `/var/lib/tessera/pam-backup/`.
- Without a fingerprint reader, no fingerprint line is written. Passkeys were removed (Nov 2026): every update takes out the pam_u2f lines, gate, key list and udev rule an older Tessera wrote for them. Face unlock never touches a login screen's PAM file.

**After every update, a boot check.** It runs as root at the end of the system update and of Tessera's own root step. It checks that:
- the login screen's PAM files work;
- greetd's config parses, and its command and user exist;
- Hyprland runs;
- Tessera's login screen's files are there.

If one of Tessera's own changes is the problem, it's undone straight away and checked again. Software Update and a notification then say **"Don't log out or restart yet"** if anything is still wrong. Settings → Health → Login screen shows the same thing, with a Fix.

**At boot, if the login screen fails anyway.**
- When Tessera's login screen ends twice in a row without a login, greetd's text login takes over in the same session (`hypr/greeter-session.sh`).
- When greetd itself keeps failing, `tessera-login-rescue.service` steps in. It goes one step further each time greetd fails during that boot:
  1. Tessera's lines come out of greetd's PAM files.
  2. greetd's text login replaces Tessera's login screen.
  3. A console login starts on tty1.

  Each step is written in the note that you see at your next login.
- After a switch from another login screen, the one from before comes back by itself if Tessera's doesn't show (`login-fallback`, `login-check`).

## What Tessera writes that the start of the computer depends on

| File | Risk | Protection |
|---|---|---|
| `/etc/pam.d/greetd` (also `ly`, `sddm`, `lightdm`, `lxdm`) | A bad line stops the login screen starting (22 Nov), or skips the password | Dashed `sufficient`/`optional` lines and the safe gate; `pam_setcred` on a copy plus the password path read before every write; backups; boot check after updates; login rescue at boot |
| `/etc/pam.d/sudo`, `polkit-1` | You can't get admin rights to repair things | The same checks before writing (the password path, read only); boot check; a console login and `su` still work |
| `/etc/pam.d/login` (keyring, installer) | The console login, the last way in | Two dashed `optional` lines only, checked the same way; kept as `.pre-tessera` |
| `/etc/greetd/config.toml` | No login screen | A fixed text; `.pre-tessera` kept; boot check; the text login from its own file and drop-in (`tessera-text-greeter.toml`, `zz-tessera-text-greeter.conf`) |
| `/usr/share/tessera` (login screen copy) | The login screen crashes | The QML login screen, then the text style (`greeter-start.sh`), then agreety (`greeter-session.sh`); boot check |
| `display-manager.service` (login switch) | Neither login manager on | Never both, never neither (`set_dm`); the one before comes back by itself (`login-fallback`, `login-check`) |
| greetd drop-ins and Tessera's units (`tessera-fallback.conf`, `tessera-rescue.conf`, `tessera-login-*`) | A unit that blocks the start | Started only on failure or by a timer, never in the boot's path; `systemd-analyze verify` in the tests; removed by `uninstall.sh` |
| `/etc/systemd/system/plymouth-quit.service.d/tessera.conf` | The splash covers the screen | `ExecStart=-…`: a failure is ignored; Ctrl+Alt+F2 always switches to a console |
| Plymouth theme, `plymouthd.conf`, `mkinitcpio -P` | A boot image that can't start | Boot images copied first; when the rebuild fails, the images and the theme setting are put back (`bootsplash`); Tessera's themes are only its own checked files |
| `/etc/default/grub` (`quiet`), `grub-mkconfig` | A boot menu that can't be written | Kept as `.pre-tessera`, put back when `grub-mkconfig` fails |
| grub-btrfs (`bootmenu`) | — | Only its own submenu file; the main menu isn't touched |
| `/etc/modprobe.d`, `/etc/modules-load.d/tessera-phonecam.conf` | A missing module at start | Only v4l2loopback's options; a missing module makes systemd-modules-load warn, and the start goes on |
| `/etc/NetworkManager/conf.d/wifi_backend.conf` (installer) | No Wi-Fi | Only when iwd already runs the Wi-Fi; never stops the start |
| `/etc/pacman.d/tessera-hold.conf`, `/etc/pacman.conf` | Updates held | Written before the Include line; not on the boot's path |
| `/etc/polkit-1/rules.d`, `/etc/ananicy.d`, `/etc/bluetooth/main.conf`, `/etc/howdy/config.ini` | — | Not on the login's path |

Tessera doesn't write `fstab`, `/etc/environment`, sysctl, the kernel command line (other than GRUB's `quiet`), or limine's or systemd-boot's entries.
