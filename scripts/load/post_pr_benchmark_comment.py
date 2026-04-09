#!/usr/bin/env python3
import json
import os
import pathlib
import urllib.error
import urllib.request


def read(path: pathlib.Path) -> str:
    return path.read_text(encoding="utf-8")


def gh_api_request(url: str, token: str, method: str = "GET", payload: dict | None = None) -> dict:
    data = None
    if payload is not None:
        data = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(url=url, method=method, data=data)
    req.add_header("Authorization", f"Bearer {token}")
    req.add_header("Accept", "application/vnd.github+json")
    req.add_header("X-GitHub-Api-Version", "2022-11-28")
    if payload is not None:
        req.add_header("Content-Type", "application/json")
    with urllib.request.urlopen(req) as resp:
        return json.loads(resp.read().decode("utf-8"))


def main() -> int:
    event_name = os.getenv("GITHUB_EVENT_NAME", "")
    if event_name != "pull_request":
        print("[bench-comment] not a pull_request event, skipping")
        return 0

    token = os.getenv("GITHUB_TOKEN")
    repo = os.getenv("GITHUB_REPOSITORY")
    event_path = os.getenv("GITHUB_EVENT_PATH")
    if not token or not repo or not event_path:
        print("[bench-comment] missing required GitHub env vars, skipping")
        return 0

    report_path = pathlib.Path("benchmarks/reports/latest.md")
    if not report_path.exists():
        print("[bench-comment] report file not found, skipping comment")
        return 0

    event = json.loads(pathlib.Path(event_path).read_text(encoding="utf-8"))
    pr_number = event.get("pull_request", {}).get("number")
    if not pr_number:
        print("[bench-comment] no pull request number in event, skipping")
        return 0

    owner, name = repo.split("/", 1)
    body = (
        "<!-- nestrs-benchmark-comment -->\n"
        "## Benchmark Report\n\n"
        + read(report_path)
    )

    comments_url = f"https://api.github.com/repos/{owner}/{name}/issues/{pr_number}/comments"
    try:
        comments = gh_api_request(comments_url, token)
    except urllib.error.HTTPError as e:
        print(f"[bench-comment] failed to list comments: {e}")
        return 0

    marker = "<!-- nestrs-benchmark-comment -->"
    existing = None
    if isinstance(comments, list):
        for c in comments:
            if marker in c.get("body", ""):
                existing = c
                break

    try:
        if existing:
            comment_id = existing["id"]
            url = f"https://api.github.com/repos/{owner}/{name}/issues/comments/{comment_id}"
            gh_api_request(url, token, method="PATCH", payload={"body": body})
            print(f"[bench-comment] updated benchmark comment id={comment_id}")
        else:
            gh_api_request(comments_url, token, method="POST", payload={"body": body})
            print("[bench-comment] created benchmark comment")
    except urllib.error.HTTPError as e:
        print(f"[bench-comment] failed to post/update comment: {e}")
        return 0

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
