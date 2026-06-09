#!/usr/bin/env python3
"""corelink-runners session fence — makes it IMPOSSIBLE for work in this repo to
write into ANY sibling HuGR project (corelink-server, hugit, hugr-wallet, …),
which frequently have their own live sessions. Owner mandate: "zero interference".

Default-deny the whole ~/Documents/HuGR/ parent except THIS repo
(`corelink-runners`). Bash referencing a sibling is allowed ONLY if the whole
command is a single, composition-free, allow-listed read-only invocation.
Fail-closed: any ambiguity / parse failure / unreadable input => DENY.

(Mirrors the hardened hugit fence. `--selftest` runs the bypass vectors.)
Exit 2 = deny (stderr shown to the model).
"""
import json
import os
import re
import shlex
import sys

_HOME = os.path.expanduser("~")
REAL_PARENT = "/Users/gustavoschneiter/Documents/HuGR/"
PARENT_FORMS = [REAL_PARENT, "~/Documents/HuGR/", "$HOME/Documents/HuGR/"]
SELF_SEG = "corelink-runners"  # the ONLY child of the parent this repo may mutate
ALLOWED_SEGS = {SELF_SEG}

READONLY_PROGS = {
    "cat", "ls", "grep", "rg", "head", "tail", "wc", "stat", "file", "diff",
    "less", "more", "column", "cut", "sort", "uniq", "nl", "tr", "comm",
    "shasum", "sha256sum", "md5", "md5sum", "basename", "dirname", "realpath",
    "readlink", "du", "tree", "pwd", "echo", "true", "test",
}
READONLY_GIT = {
    "log", "show", "diff", "status", "blame", "cat-file", "ls-files",
    "ls-tree", "rev-parse", "rev-list", "describe", "for-each-ref", "shortlog",
    "name-rev", "whatchanged", "grep", "show-ref", "symbolic-ref", "merge-base",
    "var", "count-objects", "verify-pack", "cherry",
}
COMPOSITION = ["&&", "||", ";", "|", "$(", "`", ">", "<", "\n", "&", "${", "$["]


def deny(msg: str) -> None:
    print("corelink-runners session fence: " + msg, file=sys.stderr)
    sys.exit(2)


def _normalize(path: str) -> str:
    p = path.replace("$HOME", _HOME)
    if p.startswith("~/"):
        p = _HOME + p[1:]
    return p


def write_path_forbidden(path: str) -> bool:
    p = _normalize(path)
    if not p.startswith(REAL_PARENT):
        return False
    seg = p[len(REAL_PARENT):].split("/", 1)[0]
    return seg not in ALLOWED_SEGS


def sibling_refs(cmd: str):
    refs = set()
    for form in PARENT_FORMS:
        start = 0
        while True:
            i = cmd.find(form, start)
            if i == -1:
                break
            after = cmd[i + len(form):]
            m = re.match(r"([A-Za-z0-9._-]+)", after)
            seg = m.group(1) if m else ""
            if seg not in ALLOWED_SEGS:
                refs.add(form + (seg or "<bare-parent>"))
            start = i + len(form)
    return refs


def is_readonly_git(args) -> bool:
    j = 0
    while j < len(args):
        a = args[j]
        if a in ("-C", "-c"):
            j += 2
            continue
        if a.startswith(("--git-dir", "--work-tree", "--namespace")):
            j += 1
            continue
        if a in ("-p", "--paginate", "--no-pager", "--no-replace-objects",
                 "--bare", "--literal-pathspecs"):
            j += 1
            continue
        if a.startswith("-"):
            j += 1
            continue
        break
    if j >= len(args):
        return False
    sub, rest = args[j], args[j + 1:]
    if sub in READONLY_GIT:
        return True
    if sub == "branch":
        write = ("-d", "-D", "-m", "-M", "--set-upstream-to", "-u",
                 "--edit-description", "--unset-upstream", "-c", "-C", "--move")
        return not any(f in rest for f in write)
    if sub == "tag":
        return any(f in rest for f in ("-l", "--list")) or len(rest) == 0
    if sub == "config":
        return any(f in rest for f in ("--get", "--get-all", "--get-regexp",
                                       "--list", "-l")) and not any(
            f in rest for f in ("--add", "--unset", "--unset-all",
                                "--replace-all", "--remove-section"))
    if sub == "remote":
        return len(rest) == 0 or rest[0] in ("-v", "--verbose", "show", "get-url")
    if sub == "worktree":
        return len(rest) >= 1 and rest[0] == "list"
    if sub == "reflog":
        return len(rest) >= 1 and rest[0] == "show"
    if sub == "stash":
        return len(rest) >= 1 and rest[0] in ("list", "show")
    if sub == "submodule":
        return len(rest) >= 1 and rest[0] in ("status", "foreach")
    return False


def bash_is_readonly(cmd: str) -> bool:
    if any(tok in cmd for tok in COMPOSITION):
        return False
    try:
        toks = shlex.split(cmd)
    except ValueError:
        return False
    i = 0
    while i < len(toks) and re.match(r"^[A-Za-z_][A-Za-z0-9_]*=", toks[i]):
        i += 1
    if i >= len(toks):
        return True
    prog = toks[i]
    if prog == "git":
        return is_readonly_git(toks[i + 1:])
    return prog in READONLY_PROGS


def evaluate(tool: str, ti: dict):
    if tool in ("Edit", "Write", "NotebookEdit"):
        path = ti.get("file_path") or ti.get("notebook_path") or ""
        if write_path_forbidden(path):
            return (f"writing into a sibling HuGR project is FORBIDDEN ({path}). "
                    "Only paths under .../HuGR/corelink-runners/ are writable.")
        return None
    if tool == "Bash":
        cmd = (ti.get("command") or "").strip()
        refs = sibling_refs(cmd)
        if not refs:
            return None
        if bash_is_readonly(cmd):
            return None
        return (f"command references a sibling project ({sorted(refs)}) and is not a "
                "single composition-free read-only invocation. BLOCKED (fail-closed).")
    return None


def main() -> None:
    try:
        data = json.load(sys.stdin)
    except Exception:
        deny("unreadable hook input - failing CLOSED.")
    reason = evaluate(data.get("tool_name", ""), data.get("tool_input") or {})
    if reason:
        deny(reason)
    sys.exit(0)


def _selftest() -> None:
    SIB = REAL_PARENT + "corelink-server"
    HUG = REAL_PARENT + "hugit"
    deny_cases = [
        ("Bash", {"command": f"find {SIB} -delete"}),
        ("Bash", {"command": f"git -C {SIB} gc"}),
        ("Bash", {"command": f"cat ok && rm -rf {HUG}/x"}),
        ("Bash", {"command": f"R={SIB}; rm -rf \"$R\""}),
        ("Edit", {"file_path": f"{SIB}/src/lib.rs"}),
        ("Write", {"file_path": f"{HUG}/crates/x.rs"}),
        ("Write", {"file_path": REAL_PARENT + "newsibling/y"}),
    ]
    allow_cases = [
        ("Bash", {"command": f"git -C {SIB} log --oneline -5"}),
        ("Bash", {"command": f"cat {HUG}/CLAUDE.md"}),
        ("Bash", {"command": f"grep -rn foo {SIB}/src"}),
        ("Write", {"file_path": REAL_PARENT + "corelink-runners/docs/x.md"}),
        ("Edit", {"file_path": REAL_PARENT + "corelink-runners/CLAUDE.md"}),
        ("Bash", {"command": "ls /tmp"}),
    ]
    fails = []
    for tool, ti in deny_cases:
        if evaluate(tool, ti) is None:
            fails.append(("SHOULD DENY", tool, ti))
    for tool, ti in allow_cases:
        if evaluate(tool, ti) is not None:
            fails.append(("SHOULD ALLOW", tool, ti))
    if fails:
        for f in fails:
            print("FAIL:", f, file=sys.stderr)
        sys.exit(1)
    print(f"fence self-test OK ({len(deny_cases)} deny + {len(allow_cases)} allow)")


if __name__ == "__main__":
    if "--selftest" in sys.argv:
        _selftest()
    else:
        main()
