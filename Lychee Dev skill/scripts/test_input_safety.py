"""Offline input regression: all window, keyboard and clipboard calls are mocked."""
import unittest
from unittest.mock import patch
from automation import windows

class Clipboard:
    def __init__(self): self.text = "previous clipboard"
    def read_text(self): return self.text
    def copy_text(self, value): self.text = value
    def clear(self): self.text = None

class InputSafetyTests(unittest.TestCase):
    def test_windows_path_aliases_match(self):
        identity = windows.WindowIdentity(1, 2, r"D:\Game\Wow.exe", "test")
        self.assertTrue(identity.matches(pid=2, exe_path="d:/game/WOW.EXE"))
        self.assertFalse(identity.matches(pid=3))

    def exercise(self, draft=None, *, paste=True, copy=True, lose_focus=False, mode="foreground", escape=True):
        clipboard = Clipboard()
        state = {"open": draft is not None, "text": draft or "", "selected": False,
                 "foreground": True, "public": [], "commands": []}
        def key(vk, ctrl=False):
            if vk == 0x1b:
                if escape: state.update(open=False, text="", selected=False)
            elif vk == 0xBF:
                state["open"] = True
                state["text"] += "/"
            elif vk == windows.VK_RETURN:
                if state["open"]:
                    text = state["text"]
                    if text: state["commands" if text.startswith("/") else "public"].append(text)
                    state.update(open=False, text="", selected=False)
                else: state["open"] = True
            elif ctrl and vk == 0x41: state["selected"] = True
            elif ctrl and vk == windows.VK_V and state["open"]:
                if paste:
                    state["text"] = ("" if state["selected"] else state["text"]) + clipboard.text
                    state["selected"] = False
                if lose_focus: state["foreground"] = False
            elif ctrl and vk == 0x43 and state["open"] and state["selected"] and copy:
                clipboard.text = state["text"]
        error = None
        with patch.object(windows, "confirm_window", return_value=windows.WindowIdentity(1, 2, "D:/Game/Wow.exe", "test")), \
             patch.object(windows, "focus_window"), \
             patch.object(windows, "is_foreground", side_effect=lambda hwnd: state["foreground"]), \
             patch.object(windows, "_send_key", side_effect=key), patch.object(windows.time, "sleep"):
            try:
                windows.send_command(1, "/reload", clipboard=clipboard, mode=mode)
            except windows.InputError as exc: error = exc
        self.assertEqual(state["public"], [], "automation submitted a public chat draft")
        if paste:
            self.assertEqual(clipboard.text, "previous clipboard")
        return state, error

    def test_existing_draft_is_cancelled(self):
        state, error = self.exercise("unfinished public chat ")
        self.assertIsNone(error)
        self.assertEqual(state["commands"], ["/reload"])

    def test_ignored_escape_cannot_submit_draft(self):
        state, error = self.exercise("public draft", escape=False)
        self.assertIsNone(error)
        self.assertEqual(state["commands"], ["/reload"])

    def test_messages_stays_background_and_cancels_draft(self):
        state = {"open": True, "text": "public draft", "public": [], "commands": []}
        def post(hwnd, message, value, lparam):
            self.assertEqual(hwnd, 1)
            if message == windows.WM_KEYDOWN:
                if value == windows.VK_ESCAPE:
                    state.update(open=False, text="")
                elif value == windows.VK_RETURN:
                    if state["open"]:
                        text = state["text"]
                        if text: state["commands" if text.startswith("/") else "public"].append(text)
                        state.update(open=False, text="")
                    else: state["open"] = True
            elif message == windows.WM_CHAR:
                self.assertEqual(lparam, 1)
                if state["open"]: state["text"] += chr(value)
            return 1
        with patch.object(windows, "confirm_window", return_value=windows.WindowIdentity(1, 2, "D:/Wow.exe", "test")), \
             patch.object(windows.user32, "PostMessageW", side_effect=post), \
             patch.object(windows, "focus_window", side_effect=AssertionError("must stay background")), \
             patch.object(windows, "_send_key", side_effect=AssertionError("must not SendInput")), \
             patch.object(windows, "Clipboard", side_effect=AssertionError("must not use clipboard")), \
             patch.object(windows.time, "sleep"):
            windows.send_command(1, "/reload", mode="messages")
        self.assertEqual(state["public"], [])
        self.assertEqual(state["commands"], ["/reload"])

    def test_background_rejection_does_not_queue_submit(self):
        posted = []
        def post(hwnd, message, value, lparam):
            posted.append((message, value))
            return message != windows.WM_CHAR
        with patch.object(windows, "confirm_window", return_value=windows.WindowIdentity(1, 2, "D:/Wow.exe", "test")), \
             patch.object(windows.user32, "PostMessageW", side_effect=post), \
             patch.object(windows.time, "sleep"):
            with self.assertRaises(windows.InputError):
                windows.send_command(1, "/reload", mode="messages")
        returns = [entry for entry in posted if entry == (windows.WM_KEYDOWN, windows.VK_RETURN)]
        self.assertEqual(len(returns), 1, "failed typing must not queue final submit")
        self.assertEqual(posted[-1], (windows.WM_KEYUP, windows.VK_ESCAPE))

    def test_invalid_commands_never_focus_or_type(self):
        with patch.object(windows, "focus_window") as focus, patch.object(windows, "_send_key") as key:
            for command in ("hello", "/reload\n/dev", "/" + "a" * 255):
                with self.assertRaises(windows.InputError):
                    windows.send_command(1, command)
            focus.assert_not_called()
            key.assert_not_called()

    def test_closed_and_empty_chat(self):
        for draft in (None, ""):
            state, error = self.exercise(draft)
            self.assertIsNone(error)
            self.assertEqual(state["commands"], ["/reload"])

    def test_failed_paste_or_copy_never_submits(self):
        for options in ({"paste": False}, {"copy": False}):
            state, error = self.exercise(**options)
            self.assertIsNotNone(error)
            self.assertEqual(state["commands"], [])
            self.assertFalse(state["open"])

    def test_focus_loss_never_submits(self):
        state, error = self.exercise(lose_focus=True)
        self.assertIsNotNone(error)
        self.assertEqual(state["commands"], [])

if __name__ == "__main__": unittest.main()
