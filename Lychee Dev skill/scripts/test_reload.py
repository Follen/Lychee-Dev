"""Offline protocol tests. No input is sent to a real window."""
import argparse
import json
import tempfile
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

from automation import reload as reload_mod
from automation.session import Session


def ready(nonce="current", **changes):
    data = {"v":1,"kind":"reload","run":nonce,"status":"ready","ts":1000,
            "client":"retail","build":"12.1.0.69875"}
    data.update(changes)
    return json.dumps(data)


class Capture:
    window_closed = False
    def __init__(self, frames): self.frames = iter(frames)
    def __enter__(self): return self
    def __exit__(self, *_): pass
    def latest_roi(self): return next(self.frames, None)


class Windows:
    def __init__(self, captures): self.captures = iter(captures)
    def open_notice_capture(self, *_args, **_kwargs): return Capture(next(self.captures))
    def decode_qr(self, frame): return frame


class ReloadTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.session = Session(self.directory.name, "reload-test")
        self.args = argparse.Namespace(hwnd=1,pid=2,exe_path="Wow.exe",timeout=.01,interval=.0001)
        self.sent = []
        self.deliver = lambda _s,_w,_a,text: self.sent.append(text)

    def execute(self, windows, **kwargs):
        return reload_mod.execute(windows,self.session,self.args,self.deliver,"current",**kwargs)

    def test_marker(self):
        self.assertIsNotNone(reload_mod.marker(ready(), "current"))
        for value in (ready("old"),ready(v=2),ready(status="pending"),ready(kind="ack"),
                      ready(ts=True),ready(build=""),"[]","bad","x"*513):
            self.assertIsNone(reload_mod.marker(value,"current"))

    def test_success_stale_and_cleanup(self):
        result = self.execute(Windows([[[ready("old")],[ready()]], [[ready()],[],[]]]))
        self.assertEqual(result["status"],"reload_ready")
        self.assertEqual(self.sent,["/dev auto reload current","/dev auto unidentify current"])
        self.assertEqual(self.session.read_events()[-1]["event"],"reload_ready_cleared")
        self.assertEqual(self.execute(Windows([]),resume=True)["status"],"already_completed")
        self.assertEqual(len(self.sent),2)

    def test_no_repeat_on_timeout_then_resume(self):
        with self.assertRaises(reload_mod.ReloadError): self.execute(Windows([[]]))
        with self.assertRaises(reload_mod.ReloadError): self.execute(Windows([]))
        self.assertEqual(len(self.sent),1)
        self.execute(Windows([[[ready()]], [[],[]]]),resume=True)
        self.assertEqual(self.sent.count("/dev auto reload current"),1)

    def test_no_frame_is_not_cleanup(self):
        with self.assertRaises(reload_mod.ReloadError):
            self.execute(Windows([[[ready()]], []]))
        self.assertTrue(self.session.read_events("reload_ready_confirmed"))
        self.assertFalse(self.session.read_events("reload_ready_cleared"))
        self.execute(Windows([[[],[]]]),resume=True)
        self.assertEqual(self.sent.count("/dev auto reload current"),1)

    def test_wrong_binding_and_nonce(self):
        with self.assertRaises(reload_mod.ReloadError): self.execute(Windows([]),resume=True)
        with self.assertRaises(reload_mod.ReloadError): self.execute(Windows([[]]))
        self.args.pid=3
        with self.assertRaises(reload_mod.ReloadError): self.execute(Windows([]),resume=True)
        self.assertEqual(len(self.sent),1)

    def test_ticket_forwarding(self):
        self.execute(Windows([[[ready()]], [[],[]]]),ticket="LYCHEE-TEST-1")
        self.assertEqual(self.sent[0],"/dev auto reload current LYCHEE-TEST-1")

    def test_timeout_artifact_only_on_failure(self):
        with patch.object(reload_mod,"save_timeout_image",return_value="timeout.png") as save:
            with self.assertRaises(reload_mod.ReloadError): self.execute(Windows([[]]))
            save.assert_called_once()
            self.assertEqual(self.session.read_events("reload_ready_timeout")[-1]["screenshot"],"timeout.png")

    def test_output_flow_waits_before_read(self):
        spec=importlib.util.spec_from_file_location("reload_cli_test",Path(__file__).with_name("automation.py"))
        cli=importlib.util.module_from_spec(spec);spec.loader.exec_module(cli)
        args=argparse.Namespace(**vars(self.args),sv="unused",data_dir=self.directory.name,installation="reload-test")
        notice={"task":"probe","run":"request","ticket":"LYCHEE-TEST-1","ts":1000}
        order=[]
        with patch.object(cli,"reload_ready",side_effect=lambda *a,**k: order.append("ready")), \
             patch.object(self.session,"wait_for_saved_variables",side_effect=lambda *a,**k: order.append("disk") or True), \
             patch.object(cli,"cmd_sv",side_effect=lambda *a: order.append("read") or 0):
            self.assertEqual(cli._notice_to_sv(self.session,object(),notice,args,revision="r1",request_type="task"),0)
        self.assertEqual(order,["ready","disk","read"])
        self.assertTrue(self.session.reload_already_requested("request","LYCHEE-TEST-1"))

    def test_png_rgb(self):
        try:
            import numpy as np
        except ImportError:
            self.skipTest("optional capture dependency not installed")
        import struct,zlib
        image=np.array([[[0,0,255],[255,0,0]]],dtype=np.uint8)
        path=reload_mod.save_timeout_image(self.session,"current",image)
        data=Path(path).read_bytes()
        self.assertEqual(data[:8],b"\x89PNG\r\n\x1a\n")
        size=struct.unpack(">I",data[33:37])[0]
        self.assertEqual(zlib.decompress(data[41:41+size]),b"\x00\xff\x00\x00\x00\x00\xff")


if __name__ == "__main__": unittest.main()
