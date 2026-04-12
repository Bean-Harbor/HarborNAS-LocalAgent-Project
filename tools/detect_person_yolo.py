#!/usr/bin/env python3
"""Run a minimal YOLO person detector over one image and emit JSON.

Requires:
  python3 -m pip install --user ultralytics opencv-python-headless
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="YOLO frame detector")
    parser.add_argument("--image", required=True, help="Input image path")
    parser.add_argument("--label", default="person", help="Target label, default person")
    parser.add_argument(
        "--min-confidence",
        type=float,
        default=0.25,
        help="Minimum confidence threshold",
    )
    parser.add_argument(
        "--annotated-output",
        default="",
        help="Optional annotated image output path",
    )
    parser.add_argument(
        "--model",
        default="",
        help="Optional YOLO model path/name; falls back to HARBOR_YOLO_MODEL or yolov8n.pt",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        from ultralytics import YOLO
        import cv2
    except ImportError as exc:
        print(
            json.dumps(
                {
                    "error": (
                        "missing vision dependencies; install with "
                        "'python3 -m pip install --user ultralytics opencv-python-headless'"
                    ),
                    "detail": str(exc),
                    "python": sys.executable,
                    "cwd": str(Path.cwd()),
                    "sys_path_head": sys.path[:5],
                    "pythonpath": __import__("os").environ.get("PYTHONPATH"),
                }
            ),
            file=sys.stderr,
        )
        return 2

    image_path = Path(args.image)
    if not image_path.exists():
        print(json.dumps({"error": f"image not found: {image_path}"}), file=sys.stderr)
        return 2

    model_name = args.model or ""
    if not model_name:
        model_name = __import__("os").environ.get("HARBOR_YOLO_MODEL", "yolov8n.pt")

    model = YOLO(model_name)
    results = model.predict(
        source=str(image_path),
        conf=args.min_confidence,
        verbose=False,
    )

    detections: list[dict[str, float | str]] = []
    image = cv2.imread(str(image_path))
    if image is None:
        print(json.dumps({"error": f"failed to read image: {image_path}"}), file=sys.stderr)
        return 2

    for result in results:
        names = result.names
        boxes = result.boxes
        for box in boxes:
            cls_index = int(box.cls[0].item())
            label = names.get(cls_index, str(cls_index))
            if label != args.label:
                continue

            confidence = float(box.conf[0].item())
            x1, y1, x2, y2 = [float(v) for v in box.xyxy[0].tolist()]
            detections.append(
                {
                    "label": label,
                    "confidence": confidence,
                    "x1": x1,
                    "y1": y1,
                    "x2": x2,
                    "y2": y2,
                }
            )

            cv2.rectangle(
                image,
                (int(x1), int(y1)),
                (int(x2), int(y2)),
                (0, 255, 0),
                2,
            )
            cv2.putText(
                image,
                f"{label} {confidence:.2f}",
                (int(x1), max(20, int(y1) - 10)),
                cv2.FONT_HERSHEY_SIMPLEX,
                0.6,
                (0, 255, 0),
                2,
            )

    annotated_output = args.annotated_output.strip()
    if annotated_output:
        annotated_path = Path(annotated_output)
        annotated_path.parent.mkdir(parents=True, exist_ok=True)
        cv2.imwrite(str(annotated_path), image)
        annotated_image_path = str(annotated_path)
    else:
        annotated_image_path = ""

    print(
        json.dumps(
            {
                "detections": detections,
                "annotated_image_path": annotated_image_path or None,
            },
            ensure_ascii=False,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
