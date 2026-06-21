#!/usr/bin/env python3
import json
import math
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import noema_locomo


class NoemaLocomoRunnerTests(unittest.TestCase):
    def test_default_zode_bin_falls_back_to_debug_when_release_is_missing(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            debug_bin = root / "target" / "debug" / "zode"
            debug_bin.parent.mkdir(parents=True)
            debug_bin.write_text("")

            self.assertEqual(noema_locomo.default_zode_bin(root), debug_bin)

    def test_prompt_from_messages_preserves_system_and_user_roles(self):
        task = {
            "messages": [
                {"role": "system", "content": "Return JSON only."},
                {"role": "user", "content": "Grade this answer."},
            ]
        }

        prompt = noema_locomo.prompt_from_task(task)

        self.assertEqual(prompt, "System:\nReturn JSON only.\n\nUser:\nGrade this answer.")

    def test_answer_result_line_uses_noema_custom_id(self):
        task = {
            "custom_id": "locomo-answer-conv0_q0-top_1",
            "kind": "locomo_answer_generation",
            "messages": [{"role": "user", "content": "Where did Caroline go?"}],
        }

        line = noema_locomo.result_for_task(task, "ANSWER: A Pride march.", "", 1.25)

        self.assertEqual(line["custom_id"], "locomo-answer-conv0_q0-top_1")
        self.assertEqual(line["answer"], "ANSWER: A Pride march.")
        self.assertEqual(line["secs"], 1.25)
        self.assertEqual(line["task_fingerprint"], noema_locomo.task_fingerprint(task))
        self.assertEqual(len(line["prompt_sha256"]), 64)

    def test_judge_result_line_extracts_json_from_model_output(self):
        task = {
            "custom_id": "locomo-judge-conv0_q0-top_1",
            "kind": "locomo_judge",
        }
        stdout = 'Here is the grade:\n{"reasoning":"matches","label":"correct"}'

        line = noema_locomo.result_for_task(task, stdout, "", 0.5)

        self.assertEqual(line["custom_id"], "locomo-judge-conv0_q0-top_1")
        self.assertEqual(line["label"], "CORRECT")
        self.assertEqual(line["reasoning"], "matches")
        json.dumps(line)

    def test_resume_filters_existing_custom_ids(self):
        with tempfile.TemporaryDirectory() as td:
            output = Path(td) / "answers.jsonl"
            output.write_text(
                json.dumps({"custom_id": "locomo-answer-conv0_q0-top_1"}) + "\n"
            )
            tasks = [
                {"custom_id": "locomo-answer-conv0_q0-top_1", "kind": "locomo_answer_generation"},
                {"custom_id": "locomo-answer-conv0_q1-top_1", "kind": "locomo_answer_generation"},
            ]

            done = noema_locomo.existing_custom_ids(output)
            pending = noema_locomo.pending_tasks(tasks, done)

            self.assertEqual(done, {"locomo-answer-conv0_q0-top_1"})
            self.assertEqual(
                [task["custom_id"] for task in pending],
                ["locomo-answer-conv0_q1-top_1"],
            )

    def test_retry_empty_ignores_empty_answer_results(self):
        with tempfile.TemporaryDirectory() as td:
            output = Path(td) / "answers.jsonl"
            output.write_text(
                json.dumps(
                    {
                        "custom_id": "locomo-answer-conv0_q0-top_1",
                        "kind": "locomo_answer_generation",
                        "answer": "",
                    }
                )
                + "\n"
            )

            done = noema_locomo.completed_custom_ids(output, retry_empty=True)

            self.assertEqual(done, set())

    def test_retry_failed_ignores_provider_failure_results(self):
        with tempfile.TemporaryDirectory() as td:
            output = Path(td) / "answers.jsonl"
            output.write_text(
                json.dumps(
                    {
                        "custom_id": "locomo-answer-conv0_q0-top_1",
                        "kind": "locomo_answer_generation",
                        "answer": "zode exited with status 1",
                    }
                )
                + "\n"
            )

            done = noema_locomo.completed_custom_ids(
                output,
                retry_empty=False,
                retry_failed=True,
            )

            self.assertEqual(done, set())

    def test_resume_retry_uses_latest_result_for_duplicate_ids(self):
        with tempfile.TemporaryDirectory() as td:
            output = Path(td) / "answers.jsonl"
            output.write_text(
                json.dumps(
                    {
                        "custom_id": "locomo-answer-conv0_q0-top_1",
                        "kind": "locomo_answer_generation",
                        "answer": "A Pride march.",
                    }
                )
                + "\n"
                + json.dumps(
                    {
                        "custom_id": "locomo-answer-conv0_q0-top_1",
                        "kind": "locomo_answer_generation",
                        "answer": "zode exited with status 1",
                    }
                )
                + "\n"
            )

            done = noema_locomo.completed_custom_ids(
                output,
                retry_empty=False,
                retry_failed=True,
            )

            self.assertEqual(done, set())

    def test_empty_answer_result_is_retryable(self):
        result = {
            "custom_id": "locomo-answer-conv0_q0-top_1",
            "kind": "locomo_answer_generation",
            "answer": "",
        }

        self.assertTrue(noema_locomo.retryable_result(result))

    def test_provider_failure_answer_result_is_retryable(self):
        result = {
            "custom_id": "locomo-answer-conv0_q0-top_1",
            "kind": "locomo_answer_generation",
            "answer": "zode exited with status 1",
        }

        self.assertTrue(noema_locomo.retryable_result(result))

    def test_append_result_writes_compact_jsonl(self):
        with tempfile.TemporaryDirectory() as td:
            output = Path(td) / "answers.jsonl"

            noema_locomo.append_result(
                output,
                {"custom_id": "locomo-answer-conv0_q0-top_1", "answer": "ok"},
            )

            self.assertEqual(
                output.read_text(),
                '{"custom_id":"locomo-answer-conv0_q0-top_1","answer":"ok"}\n',
            )

    def test_progress_message_names_completed_task(self):
        message = noema_locomo.progress_message(
            completed=2,
            total=5,
            skipped=1,
            result={"custom_id": "locomo-answer-conv0_q2-top_1", "secs": 1.25},
        )

        self.assertEqual(
            message,
            "[2/5 skipped=1] locomo-answer-conv0_q2-top_1 1.25s",
        )

    def test_summarize_results_counts_latest_rows_by_kind(self):
        with tempfile.TemporaryDirectory() as td:
            output = Path(td) / "results.jsonl"
            output.write_text(
                "\n".join(
                    [
                        json.dumps(
                            {
                                "custom_id": "locomo-answer-conv0_q0-top_1",
                                "kind": "locomo_answer_generation",
                                "answer": "A Pride march.",
                            }
                        ),
                        json.dumps(
                            {
                                "custom_id": "locomo-answer-conv0_q1-top_1",
                                "kind": "locomo_answer_generation",
                                "answer": "Iced tea.",
                            }
                        ),
                        json.dumps(
                            {
                                "custom_id": "locomo-answer-conv0_q1-top_1",
                                "kind": "locomo_answer_generation",
                                "answer": "zode exited with status 1",
                            }
                        ),
                        json.dumps(
                            {
                                "custom_id": "locomo-judge-conv0_q0-top_1",
                                "kind": "locomo_judge",
                                "label": "CORRECT",
                                "reasoning": "matches",
                            }
                        ),
                        json.dumps(
                            {
                                "custom_id": "locomo-judge-conv0_q1-top_1",
                                "kind": "locomo_judge",
                                "label": "WRONG",
                                "reasoning": "zode judge output did not contain a JSON object",
                            }
                        ),
                    ]
                )
                + "\n"
            )

            summary = noema_locomo.summarize_results(output)

            self.assertEqual(summary["rows"], 5)
            self.assertEqual(summary["unique"], 4)
            self.assertEqual(summary["answers"]["valid"], 1)
            self.assertEqual(summary["answers"]["failed"], 1)
            self.assertEqual(summary["answers"]["retryable"], 1)
            self.assertEqual(summary["judges"]["valid"], 1)
            self.assertEqual(summary["judges"]["retryable"], 1)

    def test_summarize_results_groups_provider_failure_reasons(self):
        with tempfile.TemporaryDirectory() as td:
            output = Path(td) / "results.jsonl"
            output.write_text(
                json.dumps(
                    {
                        "custom_id": "locomo-answer-conv0_q0-top_1",
                        "kind": "locomo_answer_generation",
                        "answer": "zode exited with status 1",
                        "stderr": "HTTP 402 Payment Required: Insufficient Balance",
                    }
                )
                + "\n"
            )

            summary = noema_locomo.summarize_results(output)

            self.assertEqual(
                summary["answers"]["failure_reasons"],
                {"http_402_payment_required": 1},
            )

    def test_summarize_results_groups_judge_provider_failure_reasons(self):
        with tempfile.TemporaryDirectory() as td:
            output = Path(td) / "results.jsonl"
            output.write_text(
                json.dumps(
                    {
                        "custom_id": "locomo-judge-conv0_q0-top_1",
                        "kind": "locomo_judge",
                        "label": "WRONG",
                        "reasoning": "zode judge output did not contain a JSON object",
                        "stderr": "HTTP 402 Payment Required: Insufficient Balance",
                    }
                )
                + "\n"
            )

            summary = noema_locomo.summarize_results(output)

            self.assertEqual(
                summary["judges"]["failure_reasons"],
                {"http_402_payment_required": 1},
            )

    def test_provider_blocker_reason_detects_judge_provider_failure(self):
        result = {
            "custom_id": "locomo-judge-conv0_q0-top_1",
            "kind": "locomo_judge",
            "label": "WRONG",
            "reasoning": "zode judge output did not contain a JSON object",
            "stderr": "HTTP 402 Payment Required: Insufficient Balance",
        }

        self.assertEqual(
            noema_locomo.provider_blocker_reason(result),
            "http_402_payment_required",
        )

    def test_summary_line_reports_retryable_counts(self):
        summary = {
            "rows": 5,
            "unique": 4,
            "answers": {"retryable": 1},
            "judges": {"retryable": 2},
        }

        line = noema_locomo.summary_line(Path("/tmp/results.jsonl"), summary)

        self.assertEqual(
            line,
            "summary /tmp/results.jsonl rows=5 unique=4 answer_retryable=1 judge_retryable=2",
        )

    def test_summary_only_cli_does_not_require_tasks(self):
        with tempfile.TemporaryDirectory() as td:
            output = Path(td) / "answers.jsonl"
            summary = Path(td) / "summary.json"
            output.write_text(
                json.dumps(
                    {
                        "custom_id": "locomo-answer-conv0_q0-top_1",
                        "kind": "locomo_answer_generation",
                        "answer": "zode exited with status 1",
                    }
                )
                + "\n"
            )

            proc = subprocess.run(
                [
                    sys.executable,
                    str(Path(noema_locomo.__file__)),
                    "--summary-only",
                    "--output",
                    str(output),
                    "--summary-output",
                    str(summary),
                ],
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertEqual(proc.returncode, 0, proc.stderr)
            self.assertIn("answer_retryable=1", proc.stdout)
            self.assertEqual(json.loads(summary.read_text())["answers"]["retryable"], 1)

    def test_summary_only_cli_can_fail_on_retryable(self):
        with tempfile.TemporaryDirectory() as td:
            output = Path(td) / "answers.jsonl"
            summary = Path(td) / "summary.json"
            output.write_text(
                json.dumps(
                    {
                        "custom_id": "locomo-answer-conv0_q0-top_1",
                        "kind": "locomo_answer_generation",
                        "answer": "zode exited with status 1",
                    }
                )
                + "\n"
            )

            proc = subprocess.run(
                [
                    sys.executable,
                    str(Path(noema_locomo.__file__)),
                    "--summary-only",
                    "--fail-on-retryable",
                    "--output",
                    str(output),
                    "--summary-output",
                    str(summary),
                ],
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertEqual(proc.returncode, 2, proc.stdout)
            self.assertIn("answer_retryable=1", proc.stdout)
            self.assertEqual(json.loads(summary.read_text())["answers"]["retryable"], 1)

    def test_summary_only_cli_provider_blocker_takes_exit_code_precedence(self):
        with tempfile.TemporaryDirectory() as td:
            output = Path(td) / "answers.jsonl"
            summary = Path(td) / "summary.json"
            output.write_text(
                json.dumps(
                    {
                        "custom_id": "locomo-answer-conv0_q0-top_1",
                        "kind": "locomo_answer_generation",
                        "answer": "zode exited with status 1",
                        "stderr": "HTTP 402 Payment Required: Insufficient Balance",
                    }
                )
                + "\n"
            )

            proc = subprocess.run(
                [
                    sys.executable,
                    str(Path(noema_locomo.__file__)),
                    "--summary-only",
                    "--fail-on-retryable",
                    "--output",
                    str(output),
                    "--summary-output",
                    str(summary),
                ],
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertEqual(proc.returncode, 3, proc.stdout)
            self.assertIn("answer_retryable=1", proc.stdout)
            data = json.loads(summary.read_text())
            self.assertEqual(
                data["answers"]["failure_reasons"],
                {"http_402_payment_required": 1},
            )

    def test_summary_only_cli_writes_run_manifest(self):
        with tempfile.TemporaryDirectory() as td:
            output = Path(td) / "answers.jsonl"
            manifest = Path(td) / "manifest.json"
            output.write_text(
                json.dumps(
                    {
                        "custom_id": "locomo-answer-conv0_q0-top_1",
                        "kind": "locomo_answer_generation",
                        "answer": "zode exited with status 1",
                    }
                )
                + "\n"
            )

            proc = subprocess.run(
                [
                    sys.executable,
                    str(Path(noema_locomo.__file__)),
                    "--summary-only",
                    "--fail-on-retryable",
                    "--output",
                    str(output),
                    "--manifest-output",
                    str(manifest),
                    "--provider",
                    "deepseek-v4-pro",
                    "--model",
                    "deepseek-v4-pro",
                ],
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertEqual(proc.returncode, 2, proc.stdout)
            data = json.loads(manifest.read_text())
            self.assertEqual(data["runner"], "zode")
            self.assertEqual(data["mode"], "summary_only")
            self.assertEqual(data["provider"], "deepseek-v4-pro")
            self.assertEqual(data["model"], "deepseek-v4-pro")
            self.assertEqual(data["paths"]["output"], str(output))
            self.assertIsNone(data["paths"]["tasks"])
            self.assertEqual(data["execution"]["run"], 0)
            self.assertEqual(data["summary"]["answers"]["retryable"], 1)
            self.assertEqual(data["retryable_total"], 1)
            self.assertFalse(data["retryable_clean"])

    def test_summary_only_manifest_marks_provider_blocker_from_results(self):
        with tempfile.TemporaryDirectory() as td:
            output = Path(td) / "answers.jsonl"
            manifest = Path(td) / "manifest.json"
            output.write_text(
                json.dumps(
                    {
                        "custom_id": "locomo-answer-conv0_q0-top_1",
                        "kind": "locomo_answer_generation",
                        "answer": "zode exited with status 1",
                        "stderr": "HTTP 402 Payment Required: Insufficient Balance",
                    }
                )
                + "\n"
            )

            proc = subprocess.run(
                [
                    sys.executable,
                    str(Path(noema_locomo.__file__)),
                    "--summary-only",
                    "--output",
                    str(output),
                    "--manifest-output",
                    str(manifest),
                ],
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertEqual(proc.returncode, 0, proc.stderr)
            data = json.loads(manifest.read_text())
            self.assertTrue(data["provider_blocked"])
            self.assertEqual(data["provider_blocker_reason"], "http_402_payment_required")
            self.assertEqual(data["execution"]["unrun_due_to_provider_blocker"], 0)

    def test_run_cli_writes_manifest_with_resume_counts(self):
        with tempfile.TemporaryDirectory() as td:
            tasks = Path(td) / "tasks.jsonl"
            output = Path(td) / "answers.jsonl"
            manifest = Path(td) / "manifest.json"
            zode = Path(td) / "fake_zode.py"
            tasks.write_text(
                json.dumps(
                    {
                        "custom_id": "locomo-answer-conv0_q0-top_1",
                        "kind": "locomo_answer_generation",
                        "messages": [{"role": "user", "content": "first"}],
                    }
                )
                + "\n"
                + json.dumps(
                    {
                        "custom_id": "locomo-answer-conv0_q1-top_1",
                        "kind": "locomo_answer_generation",
                        "messages": [{"role": "user", "content": "second"}],
                    }
                )
                + "\n"
            )
            output.write_text(
                json.dumps(
                    {
                        "custom_id": "locomo-answer-conv0_q0-top_1",
                        "kind": "locomo_answer_generation",
                        "answer": "already done",
                    }
                )
                + "\n"
            )
            zode.write_text(
                "#!/usr/bin/env python3\n"
                "import sys\n"
                "print('ANSWER: generated')\n"
            )
            zode.chmod(0o755)

            proc = subprocess.run(
                [
                    sys.executable,
                    str(Path(noema_locomo.__file__)),
                    "--tasks",
                    str(tasks),
                    "--output",
                    str(output),
                    "--zode-bin",
                    str(zode),
                    "--resume",
                    "--manifest-output",
                    str(manifest),
                    "--jobs",
                    "1",
                ],
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertEqual(proc.returncode, 0, proc.stderr)
            data = json.loads(manifest.read_text())
            self.assertEqual(data["mode"], "run")
            self.assertEqual(data["paths"]["tasks"], str(tasks))
            self.assertEqual(data["execution"]["tasks_total"], 2)
            self.assertEqual(data["execution"]["skipped"], 1)
            self.assertEqual(data["execution"]["run"], 1)
            self.assertEqual(data["summary"]["answers"]["valid"], 2)
            self.assertEqual(data["input"]["task_file_bytes"], tasks.stat().st_size)
            self.assertEqual(data["input"]["tasks_loaded"], 2)
            self.assertEqual(data["input"]["prompt_chars"]["max"], len("second"))
            self.assertEqual(data["input"]["prompt_chars"]["total"], len("first") + len("second"))
            self.assertEqual(data["input"]["estimated_prompt_tokens"]["chars_per_token"], 4)
            self.assertEqual(data["input"]["estimated_prompt_tokens"]["total"], 4)
            self.assertEqual(data["input"]["estimated_prompt_tokens"]["max"], 2)
            self.assertTrue(data["retryable_clean"])

    def test_run_cli_reuses_results_with_matching_task_fingerprint(self):
        with tempfile.TemporaryDirectory() as td:
            tasks = Path(td) / "tasks.jsonl"
            output = Path(td) / "answers.jsonl"
            previous = Path(td) / "previous.jsonl"
            manifest = Path(td) / "manifest.json"
            zode = Path(td) / "fake_zode.py"
            calls = Path(td) / "calls.txt"
            task = {
                "custom_id": "locomo-answer-conv0_q0-top_1",
                "kind": "locomo_answer_generation",
                "messages": [{"role": "user", "content": "cached prompt"}],
            }
            tasks.write_text(json.dumps(task) + "\n")
            cached = {
                "custom_id": task["custom_id"],
                "kind": task["kind"],
                "answer": "cached answer",
                "task_fingerprint": noema_locomo.task_fingerprint(task),
            }
            previous.write_text(json.dumps(cached) + "\n")
            zode.write_text(
                "#!/usr/bin/env python3\n"
                "import pathlib\n"
                f"pathlib.Path({str(calls)!r}).write_text('called')\n"
                "print('fresh answer')\n"
            )
            zode.chmod(0o755)

            proc = subprocess.run(
                [
                    sys.executable,
                    str(Path(noema_locomo.__file__)),
                    "--tasks",
                    str(tasks),
                    "--output",
                    str(output),
                    "--zode-bin",
                    str(zode),
                    "--reuse-results-from",
                    str(previous),
                    "--manifest-output",
                    str(manifest),
                ],
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertEqual(proc.returncode, 0, proc.stderr)
            self.assertFalse(calls.exists())
            rows = [json.loads(line) for line in output.read_text().splitlines()]
            self.assertEqual(rows[0]["answer"], "cached answer")
            self.assertTrue(rows[0]["reused"])
            self.assertEqual(rows[0]["reused_from"], str(previous))
            data = json.loads(manifest.read_text())
            self.assertEqual(data["execution"]["reused"], 1)
            self.assertEqual(data["execution"]["run"], 0)

    def test_summary_only_manifest_can_include_task_input_stats(self):
        with tempfile.TemporaryDirectory() as td:
            tasks = Path(td) / "tasks.jsonl"
            output = Path(td) / "answers.jsonl"
            manifest = Path(td) / "manifest.json"
            tasks.write_text(
                json.dumps(
                    {
                        "custom_id": "locomo-answer-conv0_q0-top_1",
                        "kind": "locomo_answer_generation",
                        "prompt_stats": {
                            "prompt_chars": len("first prompt"),
                            "prompt_char_budget": 48000,
                            "top_k_requested": 200,
                            "retrieval_results_available": 200,
                            "retrieval_results_considered": 200,
                            "retrieval_results_in_prompt": 3,
                            "omitted_retrieval_results": 197,
                            "truncated_memories": 0,
                        },
                        "messages": [{"role": "user", "content": "first prompt"}],
                    }
                )
                + "\n"
            )
            output.write_text("")

            proc = subprocess.run(
                [
                    sys.executable,
                    str(Path(noema_locomo.__file__)),
                    "--summary-only",
                    "--tasks",
                    str(tasks),
                    "--output",
                    str(output),
                    "--manifest-output",
                    str(manifest),
                ],
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertEqual(proc.returncode, 0, proc.stderr)
            data = json.loads(manifest.read_text())
            self.assertEqual(data["mode"], "summary_only")
            self.assertEqual(data["execution"]["tasks_total"], 1)
            self.assertEqual(data["input"]["tasks_loaded"], 1)
            self.assertEqual(data["input"]["prompt_chars"]["total"], len("first prompt"))

    def test_dry_run_cli_writes_manifest_without_output_or_provider(self):
        with tempfile.TemporaryDirectory() as td:
            tasks = Path(td) / "tasks.jsonl"
            manifest = Path(td) / "manifest.json"
            missing_zode = Path(td) / "missing-zode"
            tasks.write_text(
                json.dumps(
                    {
                        "custom_id": "locomo-answer-conv0_q0-top_1",
                        "kind": "locomo_answer_generation",
                        "prompt_stats": {
                            "prompt_chars": len("first prompt"),
                            "prompt_char_budget": 48000,
                            "top_k_requested": 200,
                            "retrieval_results_available": 200,
                            "retrieval_results_considered": 200,
                            "retrieval_results_in_prompt": 3,
                            "omitted_retrieval_results": 197,
                            "truncated_memories": 0,
                        },
                        "messages": [{"role": "user", "content": "first prompt"}],
                    }
                )
                + "\n"
            )

            proc = subprocess.run(
                [
                    sys.executable,
                    str(Path(noema_locomo.__file__)),
                    "--dry-run",
                    "--tasks",
                    str(tasks),
                    "--zode-bin",
                    str(missing_zode),
                    "--manifest-output",
                    str(manifest),
                ],
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertEqual(proc.returncode, 0, proc.stderr)
            self.assertIn("dry-run", proc.stdout)
            data = json.loads(manifest.read_text())
            self.assertEqual(data["mode"], "dry_run")
            self.assertIsNone(data["paths"]["output"])
            self.assertEqual(data["execution"]["tasks_total"], 1)
            self.assertEqual(data["execution"]["run"], 0)
            self.assertEqual(data["input"]["tasks_loaded"], 1)
            self.assertEqual(data["input"]["prompt_chars"]["total"], len("first prompt"))
            self.assertEqual(
                data["input"]["estimated_prompt_tokens"]["total"],
                math.ceil(len("first prompt") / 4),
            )
            self.assertEqual(data["input"]["noema_prompt_stats"]["tasks_with_prompt_stats"], 1)
            self.assertEqual(data["input"]["noema_prompt_stats"]["prompt_char_budgets"], [48000])
            self.assertEqual(
                data["input"]["noema_prompt_stats"]["retrieval_results_in_prompt"]["total"],
                3,
            )
            self.assertEqual(
                data["input"]["noema_prompt_stats"]["omitted_retrieval_results"]["total"],
                197,
            )
            self.assertEqual(
                data["input"]["noema_prompt_stats"]["truncated_memories"]["total"],
                0,
            )
            self.assertTrue(data["retryable_clean"])

    def test_run_cli_passes_isolated_zode_config_dir(self):
        with tempfile.TemporaryDirectory() as td:
            tasks = Path(td) / "tasks.jsonl"
            output = Path(td) / "answers.jsonl"
            manifest = Path(td) / "manifest.json"
            zode = Path(td) / "fake_zode.py"
            config_dir = Path(td) / "zode-config"
            config_dir.mkdir()
            tasks.write_text(
                json.dumps(
                    {
                        "custom_id": "locomo-answer-conv0_q0-top_1",
                        "kind": "locomo_answer_generation",
                        "messages": [{"role": "user", "content": "config?"}],
                    }
                )
                + "\n"
            )
            zode.write_text(
                "#!/usr/bin/env python3\n"
                "import os\n"
                "print(os.environ.get('ZODE_CONFIG_DIR', 'missing'))\n"
            )
            zode.chmod(0o755)

            proc = subprocess.run(
                [
                    sys.executable,
                    str(Path(noema_locomo.__file__)),
                    "--tasks",
                    str(tasks),
                    "--output",
                    str(output),
                    "--zode-bin",
                    str(zode),
                    "--zode-config-dir",
                    str(config_dir),
                    "--manifest-output",
                    str(manifest),
                ],
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertEqual(proc.returncode, 0, proc.stderr)
            result = json.loads(output.read_text())
            self.assertEqual(result["answer"], str(config_dir))
            data = json.loads(manifest.read_text())
            self.assertEqual(data["paths"]["zode_config_dir"], str(config_dir))

    def test_run_cli_can_stop_on_provider_blocker(self):
        with tempfile.TemporaryDirectory() as td:
            tasks = Path(td) / "tasks.jsonl"
            output = Path(td) / "answers.jsonl"
            manifest = Path(td) / "manifest.json"
            zode = Path(td) / "fake_zode.py"
            calls = Path(td) / "calls.txt"
            tasks.write_text(
                json.dumps(
                    {
                        "custom_id": "locomo-answer-conv0_q0-top_1",
                        "kind": "locomo_answer_generation",
                        "messages": [{"role": "user", "content": "first"}],
                    }
                )
                + "\n"
                + json.dumps(
                    {
                        "custom_id": "locomo-answer-conv0_q1-top_1",
                        "kind": "locomo_answer_generation",
                        "messages": [{"role": "user", "content": "second"}],
                    }
                )
                + "\n"
            )
            zode.write_text(
                "#!/usr/bin/env python3\n"
                "import pathlib, sys\n"
                f"calls = pathlib.Path({str(calls)!r})\n"
                "calls.write_text(calls.read_text() + 'x' if calls.exists() else 'x')\n"
                "print('HTTP 402 Payment Required: Insufficient Balance', file=sys.stderr)\n"
                "sys.exit(1)\n"
            )
            zode.chmod(0o755)

            proc = subprocess.run(
                [
                    sys.executable,
                    str(Path(noema_locomo.__file__)),
                    "--tasks",
                    str(tasks),
                    "--output",
                    str(output),
                    "--zode-bin",
                    str(zode),
                    "--stop-on-provider-blocker",
                    "--manifest-output",
                    str(manifest),
                    "--retries",
                    "0",
                ],
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertEqual(proc.returncode, 3, proc.stderr)
            self.assertIn("provider_blocker=http_402_payment_required", proc.stderr)
            self.assertEqual(calls.read_text(), "x")
            rows = [json.loads(line) for line in output.read_text().splitlines()]
            self.assertEqual(len(rows), 1)
            self.assertEqual(rows[0]["custom_id"], "locomo-answer-conv0_q0-top_1")
            data = json.loads(manifest.read_text())
            self.assertTrue(data["provider_blocked"])
            self.assertEqual(data["provider_blocker_reason"], "http_402_payment_required")
            self.assertEqual(data["execution"]["tasks_total"], 2)
            self.assertEqual(data["execution"]["pending_before_run"], 2)
            self.assertEqual(data["execution"]["run"], 1)
            self.assertEqual(data["execution"]["unrun_due_to_provider_blocker"], 1)


if __name__ == "__main__":
    unittest.main()
