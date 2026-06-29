import subprocess
import sys
from pathlib import Path


SCRIPT = Path("firmware/scripts/run-x4-nav-burst-diagnostic.py")


def test_dry_run_prints_camera_serial_crop_frames_and_contact_sheet(tmp_path):
    result = subprocess.run(
        [
            sys.executable,
            str(SCRIPT),
            "--dry-run",
            "--port",
            "/dev/test-serial",
            "--video-device",
            "/dev/test-video",
            "--rounds",
            "7",
            "--inter-key-ms",
            "13",
            "--output-dir",
            str(tmp_path),
        ],
        check=True,
        capture_output=True,
        text=True,
    )

    assert "/dev/test-video" in result.stdout
    assert "/dev/test-serial" in result.stdout
    assert "--rounds 7" in result.stdout
    assert "--inter-key-ms 13" in result.stdout
    assert "crop=440:770:770:250" in result.stdout
    assert "round-XX-seq-YYYY-expected-ZZ.jpg" in result.stdout
    assert "contact-sheet" in result.stdout
