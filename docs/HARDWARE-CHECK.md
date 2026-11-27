# Hardware walkthrough (prepared 22 Nov 2026)

These features were built and tested in a sandbox but never checked on real
devices, or were last checked under the QML shell before the Rust shell
replaced it. Do them on the laptop with the devices listed. Mark each one
✅ / ❌ and add a note; anything ❌ becomes a fix.

## Laptop alone (about 20 min)

- [ ] **Login and keyring:** restart. The login screen asks for your password,
      and no keyring prompt appears. Log out and in with your finger: still no
      prompt. *(new 22 Nov)*
- [ ] **Lock:** lock the screen, then unlock with your finger and with your
      password. Close the lid, open it: it's locked and unlocks normally.
- [ ] **Shake to find the pointer:** shake the mouse. The pointer grows and
      a ring appears.
- [ ] **Live captions:** play a video with speech. Captions appear about 2 s
      behind. Try all 3 placements (floating bar, island, bottom strip).
- [ ] **Dictation:** dictate a sentence into a text field.
- [ ] **Drop shelf:** drag real files from Dolphin onto the shelf, then out
      into another app.
- [ ] **Wi-Fi QR:** Control Center → Wi-Fi → share as QR. A phone camera can
      join with it.
- [ ] **Battery & Power:** the 20% prompt, the 3% sleep, lid actions, and
      changed idle timers taking effect.
- [ ] **Go back to before an update:** Settings → Software Update → Going
      back. Undo the last update, then redo it.
- [ ] **Island:** flick / × / Esc to dismiss, the Recent list, the Tessera
      group in Notification Center.
- [ ] **Popups:** a VS Code "Save changes?" dialog, the browser's file picker
      and a password prompt are all dark, in Tessera's colours. The picker
      is Dolphin's. *(new 21 Nov)*
- [ ] **Launcher folder:** the back arrow sits by the folder's name.
      *(new 22 Nov)*
- [ ] **Today options:** Tab through the dialog. The blue rings are whole.
      *(new 22 Nov)*
- [ ] **Tray menus:** right-click the Claude, Webex and Discord tray icons.
      Each menu has entries. *(fixed 21 Nov)*

## With your phone (an Android phone, about 15 min)

- [ ] **Quick Share, phone → laptop:** on the same Wi-Fi, and with no shared
      network.
- [ ] **Quick Share, laptop → phone:** both ways. Also send one big file
      (300 MB+).
- [ ] **Android phone link:** notifications, battery, and the USB watcher.
- [ ] **Calls:** a real WhatsApp or Discord call. Answer / Decline, Missed
      call, and the Call pill after answering.
- [ ] **Replies:** reply inline to a real message notification.

## With other devices

- [ ] **Fast Pair earbuds or headphones:** Fast Pair connects. Left /
      right / case levels show, and the 20% and 10% warnings appear.
      Bluetooth headphones become the sound output.
- [ ] **An external display:** per-display spaces (kept on unplug), the connect
      animation, Overview across displays, pointer boundaries, and the display's
      speakers in Control Center → Sound.
- [ ] **TV:** Screen Mirroring (Miracast), and Chromecast with a local video
      and a YouTube link.
- [ ] **AirPlay:** a HomePod or Apple TV, if you have one.

## Accounts (when convenient)

- [ ] **Google:** several accounts, editing events, offline changes.
- [ ] **iCloud:** sign in with an app-specific password, then add, edit and
      delete an event.
- [ ] **Microsoft 365:** sign in.
- [ ] **Drive / OneDrive folders:** install rclone, connect, open a file.
- [ ] **Claude Desktop:** Quick Entry stays open and looks right, with no
      wallet prompt.
- [ ] **Webex:** Super+Q closes the window and keeps it in the tray.
