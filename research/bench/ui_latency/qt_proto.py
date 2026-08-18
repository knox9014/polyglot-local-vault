"""
Qt(PySide6) 키입력 -> 검색 -> 리스트 갱신 지연 실측.

Tauri 프로토타입(tauri-latency-proto)과 같은 조건으로 맞췄다:
  - 합성 데이터 10만 건, 같은 생성 규칙(디렉터리/단어/인덱스)
  - 파일명 스코프 substring 매칭, 결과 50건 캡
  - 같은 타이핑 시퀀스(TYPED_QUERIES), 문자 단위로 하나씩

측정 범위도 맞췄다: "검색 호출 + 결과를 위젯에 반영하는 호출이 반환할 때까지".
Tauri 쪽 invoke() 왕복 측정도 실제 화면 페인트 완료까지는 안 재므로, 여기서도
QListWidget.clear()+addItems() 호출이 리턴하는 시점까지만 잰다 — 두 프레임워크
모두 "다음 화면 그리기 전까지" 백엔드+위젯모델 갱신 비용을 재는 것으로 통일.

Rust 바인딩(cxx-qt/qmetaobject)이 아니라 PySide6를 쓴 이유: 이 머신에 cmake와
Qt SDK 가 없어 cxx-qt 빌드가 안 된다. PySide6는 사전빌드 wheel이라 즉시 된다.
검색 로직 자체는 이미 Rust로 7.4ms(→ 17_MEASUREMENT_BASIS.md)를 실측해 뒀으므로,
여기서 재는 것은 "Qt 위젯 툴킷 자체가 키 입력마다 지연을 더하는가"이지 언어 성능이
아니다 — Python 인터프리터 오버헤드가 섞여 있다는 것은 결과 해석 시 감안할 것.
"""

import json
import sys
import time
from pathlib import Path

from PySide6.QtWidgets import QApplication, QWidget, QVBoxLayout, QLineEdit, QListWidget, QLabel


def synthetic_dataset(n: int) -> list[str]:
    dirs = ["src", "tests", "docs", "examples", "vendor", "lib", "scripts"]
    words = ["router", "config", "handler", "utils", "model", "view", "controller",
             "service", "parser", "index", "auth", "session", "cache", "queue",
             "worker", "client"]
    out = []
    for i in range(n):
        d = dirs[i % len(dirs)]
        w = words[(i // len(dirs)) % len(words)]
        out.append(f"{d}/module_{i // 137}/{w}_{i}.rs")
    return out


DATASET = synthetic_dataset(100_000)
TYPED_QUERIES = ["router", "config_12345", "index_99999", "vendor_worker_500", "auth"]


def search(query: str) -> list[str]:
    q = query.lower()
    out = []
    for path in DATASET:
        name = path.rsplit("/", 1)[-1]
        if q in name.lower():
            out.append(path)
            if len(out) >= 50:
                break
    return out


def percentile(sorted_samples: list[float], p: float) -> float:
    idx = min(len(sorted_samples) - 1, int((p / 100) * len(sorted_samples)))
    return sorted_samples[idx]


def main():
    app = QApplication(sys.argv)
    window = QWidget()
    layout = QVBoxLayout(window)
    status = QLabel("starting...")
    line_edit = QLineEdit()
    result_list = QListWidget()
    layout.addWidget(status)
    layout.addWidget(line_edit)
    layout.addWidget(result_list)
    window.show()

    samples: list[float] = []

    def keystroke(partial: str) -> float:
        t0 = time.perf_counter()
        results = search(partial)
        line_edit.setText(partial)
        result_list.clear()
        result_list.addItems(results)
        app.processEvents()  # let Qt actually apply the update, matching a real keystroke's cost
        return (time.perf_counter() - t0) * 1000.0

    def run_benchmark():
        status.setText(f"dataset: {len(DATASET)} files. warming up...")
        app.processEvents()
        for i in range(50):
            keystroke(f"warmup{i}")

        rounds = 30
        for r in range(rounds):
            for full in TYPED_QUERIES:
                partial = ""
                for ch in full:
                    partial += ch
                    samples.append(keystroke(partial))
            status.setText(f"round {r + 1}/{rounds}, {len(samples)} samples so far...")
            app.processEvents()

        sorted_samples = sorted(samples)
        stats = {
            "framework": "qt (pyside6 widgets, in-process — no IPC boundary)",
            "dataset_size": len(DATASET),
            "samples": len(samples),
            "p50_ms": percentile(sorted_samples, 50),
            "p95_ms": percentile(sorted_samples, 95),
            "p99_ms": percentile(sorted_samples, 99),
            "max_ms": sorted_samples[-1],
            "min_ms": sorted_samples[0],
        }
        status.setText(
            f"done. p50={stats['p50_ms']:.2f}ms p95={stats['p95_ms']:.2f}ms "
            f"p99={stats['p99_ms']:.2f}ms max={stats['max_ms']:.2f}ms"
        )
        app.processEvents()

        run = sys.argv[1] if len(sys.argv) > 1 else "1"
        out_path = Path(__file__).parent / f"results_qt_{run}.json"
        out_path.write_text(json.dumps(stats, indent=2))
        app.quit()

    from PySide6.QtCore import QTimer
    QTimer.singleShot(100, run_benchmark)  # let the window actually show first
    app.exec()


if __name__ == "__main__":
    main()
