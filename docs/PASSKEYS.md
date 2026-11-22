# Passkeys

Linux has no built-in passkey store that browsers use. What every browser
(Chrome, Firefox, Zen, Electron apps) and libfido2 (`ssh-keygen`, pam_u2f)
does support is a USB security key. So Tessera can be one: turn on
**Settings → Privacy & Security → Passkeys**, and this laptop shows up as a
security key called **Tessera Passkeys**. Each time a site, ssh or sudo wants
to use it, Tessera's prompt names the site and the account, and you approve
with your fingerprint (or your password, if the finger isn't read or you
have no reader).

It's off by default, and it's the Rust shell's (tessera-ui).

## Passkeys stay on this laptop

Your passkeys are sealed by this laptop's security chip (TPM 2.0). That's
what keeps them safe, and it means:

- they can't be copied, backed up, exported or moved to another computer;
  a copied file is useless anywhere else;
- a new laptop, a new motherboard, or clearing the TPM in the firmware
  loses them;
- so **keep another way to sign in to each site**: a password, your phone,
  another security key. Most sites let you have several.

Without a TPM 2.0 (or with it turned off in the firmware settings),
Passkeys can't be turned on, and Settings says why.

## Using it

- **Websites**: on a site's security settings, add a passkey (or "a
  security key"). The browser asks Tessera Passkeys; Tessera's prompt asks
  for your finger. Signing in is the same: pick the passkey, touch the reader.
  Try it at [webauthn.io](https://webauthn.io).
- **Older two-step keys (U2F)**: sites that only take "a security key" as a
  second step work too.
- **SSH**: `ssh-keygen -t ed25519-sk` (or `-t ecdsa-sk`) makes a key that
  lives here; ssh asks Tessera's prompt each time it's used. Add
  `-O resident` to make it discoverable (`ssh-keygen -K` lists it again),
  `-O verify-required` to have the server insist on your fingerprint.
  The key file ssh writes (`~/.ssh/id_ed25519_sk`) only names the key; it's
  useless without this laptop.
- **sudo and login** (optional, in the same Settings group): Use for sudo
  and login makes a key for `pam://tessera` (the prompt asks for your
  finger) and adds it to sudo's and the login screen's PAM files, asked
  before your password. It needs your password once. Your password always
  works as before. It's used only while you're signed in and at this
  computer (never for a login from elsewhere), and never for Tessera's own
  password prompts, which already take your finger.

Settings lists every saved passkey (the site, the account, when it was made
and last used); Delete asks for your finger first. Turning Passkeys off
stops the key and keeps the saved passkeys, sealed, for when it's on again.

## How it works

- `tessera-passkeys` (native/tessera-passkeys) is a user service
  (`~/.config/systemd/user/tessera-passkeys.service`), so it keeps running
  when the shell restarts. It makes the key through /dev/uhid: a HID device
  with FIDO's report descriptor (usage page 0xF1D0), vendor and product id
  **1209:7E55** (pid.codes' open-source vendor id; the product id is
  Tessera's own and stays the same). The kernel makes its /dev/hidrawN.
- It speaks CTAPHID, CTAP 2.0 (makeCredential, getAssertion,
  getNextAssertion, getInfo) and U2F; ES256 for sites, EdDSA too for SSH.
  Passkeys are resident (discoverable) keys. It reports user verification
  built in and no PIN: the check is your fingerprint or password in
  Tessera's prompt, so browsers never ask you to set a PIN. Attestation is
  self-attestation: no certificate that could tell this laptop from another.
- Each credential's private key is encrypted (AES-256-GCM) with a random
  wrapping key, and the wrapping key is sealed by the TPM (tpm2-tools, the
  owner hierarchy, `/dev/tpmrm0`). The files are in
  `~/.local/share/tessera/passkeys/` (0700): `wrap.sealed`, `creds.json`
  (site, account, dates, sign counter: no secrets), `keys/<id>.key`.
- The seal has **no PCR policy** on purpose: binding it to the firmware and
  boot chain would lock your passkeys away for good after a BIOS, firmware
  or bootloader update or a Secure Boot change, and there's no backup to
  come back from. It's bound to this TPM only. What that gives up: another
  system booted on this same laptop, with your home folder readable, could
  unseal it; full-disk encryption covers that.
- Every request (a new passkey, a sign-in, a Delete) is asked in Tessera's
  password dialog. The check itself is the daemon's own PAM conversation
  (a finger through pam_fprintd, then the password through pam_unix), with
  its own PAM file in its runtime folder, never the system's (pam_u2f there
  would ask this very key). The shell only shows it: nothing the shell says
  approves anything; only the check succeeding does. While you're asked,
  the key tells the browser to wait (KEEPALIVE); if the browser gives up,
  the prompt goes. With no shell running, requests are refused at once.
- The sign counter goes up with each use and is saved before the signature
  is sent.
- Turning it on (the root helper, `tessera-root passkeys on`, with your
  password) installs `tpm2-tools` and `pam-u2f` when they're missing and
  writes `/etc/udev/rules.d/70-tessera-passkeys.rules`: uaccess (for
  whoever is at this computer) on /dev/uhid, the TPM's /dev/tpmrm0 and the
  key's hidraw node. systemd's 60-fido-id.rules recognises the key as a
  security token as well.
- Use for sudo and login: `pamu2fcfg` makes the key, `tessera-root
  passkeys-pam on` checks its one line and keeps it in
  `/etc/tessera/passkeys-u2f`, then adds two marked lines (`# Tessera
  passkey`) first in `/etc/pam.d/sudo` and the login screens' files
  (greetd, ly, SDDM, LightDM, LXDM): a gate
  (`/usr/lib/tessera/passkeys-gate`: your daemon is running, and the login
  isn't from elsewhere) and `auth sufficient pam_u2f.so … userverification=1`.

## Limits

- One laptop only, no backup or sync (above).
- No PIN, and no CTAP 2.1 extras: credential management happens in
  Settings, not in a browser's "manage security key" page; hmac-secret
  (some password managers' unlock) isn't offered.
- A browser's "reset security key" is refused; delete passkeys in Settings.
- Granting uaccess on /dev/uhid lets programs you run make virtual input
  devices. Turning Passkeys off removes the rule.
- sudo and login use the key only while your session (and its daemon) is
  running: it doesn't replace the password at the first login after a
  restart.

## Checking and removing it

- Settings → About → Health has a Passkeys row while it's on (its daemon,
  the TPM, the key, the prompt), with a fix.
- `tessera-passkeys --status` and `--list` ask the running daemon;
  `journalctl --user -u tessera-passkeys` says what it did.
- **Removing it**: turn Passkeys off in Settings (the service stops, the
  udev rule and any sudo/login lines go). To delete the saved passkeys too,
  delete them one by one in Settings first, or remove
  `~/.local/share/tessera/passkeys/` (they can't be recovered; remove this
  laptop from each site's security keys as well). `./uninstall.sh` stops
  the service and removes the program, the rule and the PAM lines; it
  leaves the passkeys folder.
