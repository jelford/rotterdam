#!/usr/bin/env python3
import os
from pathlib import Path
import subprocess
import json
import sys
import contextlib
from time import sleep
from typing import Dict, Any, List
from pprint import pprint

__here = Path(__file__).parent



@contextlib.contextmanager
def _echo_server():
    if "--no-launch" in sys.argv:
        yield
        return

    try:
        subprocess.check_call(["cargo", "build", "--examples"])
        p = subprocess.Popen(["cargo", "run", "--example", "echo-server", "--quiet"], stdout=subprocess.PIPE, stderr=None, stdin=None, encoding='utf-8')
        for _ in range(10):
            print("Checking...")
            if p.poll() is not None:
                raise RuntimeError("Echo server failed")
            if p.stdout.readline().startswith("Listening on port "):
                break
            sleep(0.01)
        else:
            raise RuntimeError("Never got listening message from server")
        yield
    finally:
        p.terminate()
        p.wait()


def _normalize(from_json: Any) -> Any:
    if isinstance(from_json, dict):
        return _normalize_object(from_json)
    elif isinstance(from_json, list):
        return _normalize_list(from_json)
    else:
        return from_json


def _normalize_object(from_json: Dict[str, Any]) -> Dict[str, Any]:
    result = {k: v for (k, v) in sorted(from_json.items())}
    for k, v in result.items():
        result[k] = _normalize(v)

    return result

def _normalize_list(from_json: List[Any]) -> List[Any]:
    return sorted([_normalize(v) for v in from_json], key=json.dumps)


def run():
    test_cases = __here.glob("test-*.request")
    fail = None
    with _echo_server():
        for test_case in test_cases:
            with test_case.open("r", encoding="utf-8") as f:
                command = f.readline().strip()
                expected_raw = f.read()

            if not expected_raw:
                expected_raw = "{}"
                fail = f"Expectation not set for {test_case}"

            expected = _normalize(json.loads(expected_raw))
            actual = _normalize(json.loads(subprocess.check_output(f"{command} | jq -S", shell=True)))
            
            if actual != expected:
                print(f"Test case failed ({test_case.name})", file=sys.stderr)
                print("Expected:", file=sys.stderr)
                print(json.dumps(expected, indent=4), file=sys.stderr)
                print("\nGot:", file=sys.stderr)
                print(json.dumps(actual, indent=4), file=sys.stderr)
        
    sys.exit(fail)
        

if __name__ == "__main__":
    run()