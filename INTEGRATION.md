# Integrating with mitos-session

## 1. Boot checklist

What the image has to provide before mitos-services starts `mitos-session`.

| Needs | Why |
|---|---|
| Started as **root** by mitos-services | It drops privileges itself, per session, when it launches the compositor. It reports readiness with `sd_notify` (`READY=1`) once its socket is bound. |
| `/run` writable (tmpfs) | The IPC socket (`/run/mitos-session/session.sock`) and each user's `XDG_RUNTIME_DIR` (`/run/mitos-session/<uid>`) are created there. |
| `/etc/pam.d/mitos-session` and `/etc/pam.d/mitos-elevate` | PAM services for lock-screen unlock and elevation prompts. Samples are in `config/pam.d/`. |
| A group every desktop user belongs to, named in `[ipc].socket_group` | The compositor runs as the logged-in user and must reach the socket. Left unset, only root can connect. |
| `libpam.so.0` and `libudev.so.1` | Dynamic dependencies of the binary (PAM checks, device enumeration). |
| `mitos-gui` at `[session].compositor_binary` (default `/usr/bin/mitos-gui`) | Spawned for every non-tty session. |
| mitos-power listening on `[power].mitos_power_socket_path`, **or** `[power].backend = "direct"` | The default power backend asks mitos-power to do the actual suspend/reboot/poweroff. |
| An account named by `[elevation].service_user` (default `mitos-service`) | Only that account, or root, may open elevation prompts. |
| `/etc/mitos/session.toml` | Optional. Every key has a compiled-in default; `config/session.toml` lists them. |

`/var/log/mitos` (audit log) is created on startup if it is missing.
Send `SIGHUP` to re-read the config file the daemon was started with.

## 2. Integration by project
🖥️  mitos-gui  (The Compositor & Shell)
 mitos-gui  is the most critical client. It manages the Wayland display, renders the lock screen, and handles input routing.
Lifecycle:
	1.	Spawn: Launched by  mitos-session  as a child process with dropped privileges.
	2.	Register: Immediately sends  Request::RegisterCompositor { session_id }  to tell the daemon the display is up.
	3.	State Sync: Listens for  Event::SessionStateChanged . If  locked == true ,  mitos-gui  MUST obscure all normal client surfaces and render the lock screen UI.
	4.	Permission Checks: Before granting an app access to screen capture or global shortcuts,  mitos-gui  MUST send  Request::CheckPermission { session_id, app_uid, permission }  to the daemon. The daemon will return  PermissionDenied  if the session is locked.
Key Events to Handle:
	•	 ShowLockScreen { reason } : Render the lock UI.
	•	 HideLockScreen : Authenticate succeeded, restore the desktop.
	•	 PrepareForSleep : The hardware is about to suspend. Flush GPU buffers and pause rendering.
	•	 Dim : The idle timer fired; reduce screen brightness.
🔐  mitos-login  (The Greeter)
Runs as root (or a dedicated greeter user) before any user session exists.
Workflow:
	1.	Queries available users:  Request::ListAccounts  ->  Response::Accounts(Vec<Account>) .
	2.	Queries hardware state:  Request::GetSystemStatus  ->  Response::SystemStatus  (to show battery/network icons).
	3.	Queries session types:  Request::ListSessionTypes  ->  Response::SessionTypes  (Wayland/X11).
	4.	When the user clicks “Login”, it sends  Request::CreateSession { user_name, seat_id, session_type } .
	5.	If  CreateSession  succeeds,  mitos-login  exits, and  mitos-session  spawns  mitos-gui .
🛡️  mitos-service  (The Policy/Elevation Daemon)
The MITOS equivalent of  polkit . When an app wants to modify  /etc/fstab  or install a package, it asks  mitos-service .
Elevation Workflow:
	1.	 mitos-service  verifies the action is allowed by policy.
	2.	It sends  Request::RequestElevation { session_id, action }  to  mitos-session .
	3.	 mitos-session  pushes  Event::ShowElevationPrompt  to  mitos-gui .
	4.	 mitos-gui  renders a secure password dialog (isolated from normal apps) and sends  Request::RespondElevation { request_id, response: Password(...) }  back to  mitos-session .
	5.	 mitos-session  verifies the password via PAM and replies to  mitos-service  with  AuthResult(Success) .
🎵  mitos-audio  & 🌐  mitos-network  (Status Broadcasters)
These system services run as root and monitor hardware state. They do not need to ask for permission; they push state updates.
Workflow:
	1.	NetworkManager detects Wi-Fi disconnect.
	2.	 mitos-network  sends  Request::UpdateSystemStatus(SystemStatus { network_online: false, ... }) .
	3.	 mitos-session  broadcasts  Event::SystemStatusChanged  to all connected  mitos-gui  instances so the system tray updates instantly.
🔔  mitos-notifications  (The Notification Server)
Workflow:
	1.	Listens for  Event::NotificationPolicyChanged .
	2.	If  policy.redact_bodies == true  (which happens automatically when the session locks), the notification server MUST strip the body text from all incoming desktop notifications and only show the app name (e.g., “Signal - New Message” instead of the actual message content).



## 3. API reference summary
Requests (Client -> Daemon)
	•	 CreateSession { user_name, seat_id, session_type } 
	•	 TerminateSession { session_id } 
	•	 RegisterCompositor { session_id } 
	•	 LockSession { session_id } 
	•	 Unlock { session_id, user_name, password }  (Note: the password is a plain  String  on the wire -- clients should clear it from memory as soon as it has been sent)
	•	 CheckPermission { session_id, app_uid, permission } 
	•	 RequestElevation { session_id, action } 
	•	 RespondElevation { request_id, response } 
	•	 ListAccounts ,  ListSessionTypes ,  GetSystemStatus 
	•	 UpdateSystemStatus(SystemStatus)  (Root only)
Events (Daemon -> Client)
	•	 SessionStateChanged { session_id, state, locked } 
	•	 ShowLockScreen { session_id, reason } 
	•	 HideLockScreen { session_id } 
	•	 PrepareForSleep 
	•	 NotificationPolicyChanged(NotificationPolicy) 
	•	 SystemStatusChanged(SystemStatus) 
	•	 ShowElevationPrompt { request_id, session_id, action } 
	•	 AuthFeedback { session_id, outcome }
