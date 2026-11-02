#!/usr/bin/env python3
"""Utui helper for account-synced likes and Watch Later.

Utui does not ship an HTTP stack for YouTube's logged-in web API, so this
script rides on yt-dlp: yt-dlp extracts the browser cookies (including the
keyring dance for Chrome, the sqlite dance for Firefox, ...), and this
helper signs YouTube innertube requests with SAPISIDHASH the same way the
web client does. One JSON result line is printed; exit code mirrors it.

Supported actions:
  sync.py <browser-spec> like <video-id> on|off
  sync.py <browser-spec> watchlater <video-id> add|remove
  sync.py <browser-spec> check
  sync.py <browser-spec> account

`browser-spec` accepts whatever `yt-dlp --cookies-from-browser` accepts,
e.g. "firefox", "chrome", "firefox::/path/to/profile", "brave:yand2fz:".
"""
import hashlib
import json
import re
import sys
import time

ORIGIN = "https://www.youtube.com"
CLIENT_FALLBACK = {"key": "AIzaSyAO_FJ2SlqU8Q4STEHLGCilw_Y9_11qcW8", "version": "2.20260101.00.00"}

USAGE = (
    "usage: sync.py <browser> like <id> on|off | "
    "sync.py <browser> watchlater <id> add|remove | "
    "sync.py <browser> check | sync.py <browser> account"
)


def emit(ok, **fields):
    print(json.dumps({"ok": ok, **fields}))
    sys.exit(0 if ok else 1)


def youtube_cookies(ydl):
    bucket = {}
    for cookie in ydl.cookiejar:
        domain = cookie.domain or ""
        if "youtube.com" not in domain:
            continue
        if cookie.name in ("SAPISID", "__Secure-1PAPISID", "__Secure-3PAPISID") and cookie.value:
            bucket.setdefault(cookie.name, cookie.value)
    return bucket


def authorization(sids, now):
    """Mirror yt-dlp's YouTube SAPISIDHASH construction."""
    primary = sids.get("SAPISID") or sids.get("__Secure-3PAPISID")
    entries = []
    if primary:
        entries.append(("SAPISIDHASH", primary))
    if sids.get("__Secure-1PAPISID"):
        entries.append(("SAPISID1PHASH", sids["__Secure-1PAPISID"]))
    if sids.get("__Secure-3PAPISID"):
        entries.append(("SAPISID3PHASH", sids["__Secure-3PAPISID"]))
    parts = []
    for scheme, sid in entries:
        digest = hashlib.sha1(f"{now} {sid} {ORIGIN}".encode()).hexdigest()
        parts.append(f"{scheme} {now}_{digest}")
    return " ".join(parts)


def web_client(ydl):
    """Live clientVersion / INNERTUBE key from the web front page."""
    try:
        body = ydl.urlopen("https://www.youtube.com/").read().decode("utf-8", "ignore")
        key = re.search(r'"INNERTUBE_API_KEY"\s*:\s*"([^"]+)"', body)
        version = re.search(r'"clientVersion"\s*:\s*"([\d.]+)"', body)
        if key and version:
            return key.group(1), version.group(1)
    except Exception:
        pass
    return CLIENT_FALLBACK["key"], CLIENT_FALLBACK["version"]


def innertube(ydl, path, body):
    """POST one innertube call with the WEB client's request context.

    Every call needs the context, and playlist edits in particular are
    refused with FAILED_PRECONDITION without it — the failure that made
    Watch Later removals bounce before.
    """
    key, version = web_client(ydl)
    headers = {
        "Content-Type": "application/json",
        "X-Goog-Api-Format-Version": "2",
        "X-Youtube-Client-Name": "1",
        "X-Youtube-Client-Version": version,
        "X-Origin": ORIGIN,
        "X-Goog-AuthUser": "0",
        "Origin": ORIGIN,
        "Referer": f"{ORIGIN}/",
        "Authorization": authorization(youtube_cookies(ydl), str(int(time.time()))),
    }
    payload = {
        "context": {
            "client": {"clientName": "WEB", "clientVersion": version, "hl": "en"},
            "user": {"lockedSafetyMode": False},
            "request": {"useSsl": True},
        },
        **body,
    }
    url = f"https://www.youtube.com/youtubei/v1/{path}?prettyPrint=false&key={key}"
    from yt_dlp.networking import Request

    response = ydl.urlopen(Request(url, data=json.dumps(payload).encode(), headers=headers))
    text = response.read().decode("utf-8", "ignore")
    return json.loads(text or "{}")


def find_account_name(value):
    """Recursive search for the signed-in account's display name.

    account_menu nests it in a few shapes across client versions: a plain
    string, or a `runs` list. Mirror ytmtui and take the first hit.
    """
    if isinstance(value, dict):
        name = value.get("accountName")
        if isinstance(name, str) and name:
            return name
        if isinstance(name, dict):
            runs = name.get("runs") or []
            if runs and isinstance(runs[0].get("text"), str):
                return runs[0]["text"]
        for child in value.values():
            found = find_account_name(child)
            if found:
                return found
    elif isinstance(value, list):
        for child in value:
            found = find_account_name(child)
            if found:
                return found
    return None


def main(argv):
    if len(argv) < 2:
        emit(False, error=USAGE)
    browser, mode = argv[0], argv[1]

    from yt_dlp import YoutubeDL

    try:
        ydl = YoutubeDL(
            {
                "quiet": True,
                "no_warnings": True,
                "socket_timeout": 20,
                "cookiesfrombrowser": tuple(browser.split(":")),
            }
        )
        cookies = youtube_cookies(ydl)
        if not cookies:
            emit(False, error="not signed in: no SAPISID cookie found in the browser jar")
    except Exception as error:
        emit(False, error=f"could not read browser cookies: {error}")

    if mode == "check":
        emit(True, signed_in=True)

    if mode == "account":
        try:
            data = innertube(ydl, "account/account_menu", {})
        except Exception as error:
            emit(False, error=str(error) or type(error).__name__)
        name = find_account_name(data)
        if name:
            emit(True, account=name)
        emit(False, error="no account name in the account_menu response")

    if len(argv) < 4:
        emit(False, error=USAGE)
    video_id, state = argv[2], argv[3]
    if not re.fullmatch(r"[A-Za-z0-9_-]{5,20}", video_id):
        emit(False, error=f"not a video id: {video_id!r}")

    try:
        if mode == "like":
            if state not in ("on", "off"):
                emit(False, error=USAGE)
            path = "like/like" if state == "on" else "like/removelike"
            data = innertube(ydl, path, {"target": {"videoId": video_id}})
            if str(data.get("status", "")).startswith("STATUS_FAIL"):
                emit(False, error=f"YouTube rejected the like action: {data}")
            emit(True, action=f"like {state}")
        elif mode == "watchlater":
            if state not in ("add", "remove"):
                emit(False, error=USAGE)
            if state == "add":
                actions = [
                    {
                        "action": "ACTION_ADD_VIDEO",
                        "addedVideoId": video_id,
                        "actionSource": "ACTION_SOURCE_WATCH_LATER",
                    }
                ]
            else:
                actions = [
                    {"action": "ACTION_REMOVE_VIDEO_BY_VIDEO_ID", "removedVideoId": video_id}
                ]
            data = innertube(
                ydl, "browse/edit_playlist", {"playlistId": "WL", "actions": actions}
            )
            status = data.get("status")
            if status == "STATUS_SUCCEEDED":
                emit(True, action=f"watchlater {state}")
            emit(False, error=f"unexpected edit_playlist response: {json.dumps(data)[:200]}")
        else:
            emit(False, error=USAGE)
    except Exception as error:  # HTTPError carries the response body text
        detail = str(error)
        body = getattr(error, "response", None)
        if body is not None:
            try:
                detail = f"{detail}: {body.read().decode('utf-8', 'ignore')[:200]}"
            except Exception:
                pass
        emit(False, error=detail or type(error).__name__)


if __name__ == "__main__":
    main(sys.argv[1:])
