from __future__ import annotations
import copy,importlib.util,json,tempfile,unittest,sys
from pathlib import Path

PATH=Path(__file__).resolve().parents[1]/"report.py"
SPEC=importlib.util.spec_from_file_location("eval_report",PATH); assert SPEC and SPEC.loader
mod=importlib.util.module_from_spec(SPEC); sys.modules[SPEC.name]=mod; SPEC.loader.exec_module(mod)
FIX=Path(__file__).parent/"fixtures"/"run_bundle.json"

def materialize(root:Path,payload:dict)->Path:
    run=root/payload["metadata"]["run_id"]; (run/"sources").mkdir(parents=True)
    (run/"metadata.json").write_text(json.dumps(payload["metadata"]),encoding="utf-8")
    (run/"rubric.csv").write_text(payload["rubric_csv"],encoding="utf-8")
    for source in payload["sources"]:
        folder=run/"sources"/source["dir"]; folder.mkdir()
        (folder/"result.json").write_text(json.dumps(source["result"]),encoding="utf-8")
        if "view" in source: (folder/"view.json").write_text(json.dumps(source["view"]),encoding="utf-8")
    return run

class ReportTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls): cls.data=json.loads(FIX.read_text())
    def test_partial_failure_metrics(self):
        with tempfile.TemporaryDirectory() as tmp: report=mod.aggregate(materialize(Path(tmp),self.data["current"]))
        self.assertEqual(report["operational"]["failed_sources"],1)
        self.assertEqual(report["operational"]["ready_clips"],2)
        self.assertEqual(report["operational"]["duplicate_rate"],0.2)
        self.assertEqual(report["human_review"]["would_post_rate"],0.5)
        self.assertEqual(report["human_review"]["false_accepts"],1)
    def test_gate_flags_regressions(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); baseline=mod.aggregate(materialize(root,self.data["baseline"])); current=mod.aggregate(materialize(root,self.data["current"])); gate=mod.compare(current,baseline)
        self.assertEqual(gate["status"],"fail")
        self.assertTrue(any("new source failures" in x for x in gate["failure_reasons"]))
        self.assertTrue(any("would-post" in x for x in gate["failure_reasons"]))
    def test_gate_fails_for_missing_baseline_source(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); baseline=mod.aggregate(materialize(root,self.data["baseline"])); current=copy.deepcopy(baseline)
        current["sources"].append({**current["sources"][0],"source_id":"new-source"})
        gate=mod.compare(current,baseline)
        self.assertTrue(any("missing from baseline" in x for x in gate["failure_reasons"]))
    def test_gate_fails_for_new_non_complete_source_and_failed_clips(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); baseline=mod.aggregate(materialize(root,self.data["baseline"])); current=mod.aggregate(materialize(root,self.data["current"])); gate=mod.compare(current,baseline)
        self.assertTrue(any("newly non-complete" in x for x in gate["failure_reasons"]))
        self.assertTrue(any("failed clips increased" in x for x in gate["failure_reasons"]))
    def test_gate_fails_for_invalid_partial_rubric_and_incomplete_coverage(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); baseline=mod.aggregate(materialize(root,self.data["baseline"])); current=copy.deepcopy(baseline)
        current["human_review"]["invalid_rows"]=[4]
        current["human_review"]["clips_reviewed"]=2
        gate=mod.compare(current,baseline)
        self.assertTrue(any("invalid or partial" in x for x in gate["failure_reasons"]))
        self.assertTrue(any("incomplete review coverage" in x for x in gate["failure_reasons"]))
    def test_gate_fails_when_baseline_metrics_are_missing_from_current_evidence(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); baseline=mod.aggregate(materialize(root,self.data["baseline"])); current=copy.deepcopy(baseline)
        current["human_review"]["would_post_rate"]=None
        current["human_review"]["score_averages"].pop("hook_1to5")
        gate=mod.compare(current,baseline)
        self.assertTrue(any("would-post metric" in x for x in gate["failure_reasons"]))
        self.assertTrue(any("hook_1to5" in x for x in gate["failure_reasons"]))
    def test_enforce_requires_baseline(self):
        with tempfile.TemporaryDirectory() as tmp:
            run=materialize(Path(tmp),self.data["current"])
            self.assertEqual(mod.main([str(run),"--enforce"]),1)
    def test_partial_rubric_row_is_invalid(self):
        with tempfile.TemporaryDirectory() as tmp:
            path=Path(tmp)/"rubric.csv"; path.write_text("source,hook_1to5,would_post_1to5\nclip,4,5\n")
            result=mod.load_rubric(path)
        self.assertEqual(result["clips_reviewed"],0)
        self.assertEqual(result["invalid_rows"],[2])
    def test_cli_is_deterministic(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); run=materialize(root,self.data["current"])
            outputs=[]
            for prefix in ("a","b"):
                paths=[root/f"{prefix}.{ext}" for ext in ("json","md","csv")]
                self.assertEqual(mod.main([str(run),"--output-json",str(paths[0]),"--output-md",str(paths[1]),"--output-csv",str(paths[2])]),0); outputs.append([p.read_bytes() for p in paths])
        self.assertEqual(outputs[0],outputs[1])
    def test_full_selection_overrides_capped_view_summary(self):
        with tempfile.TemporaryDirectory() as tmp:
            run=materialize(Path(tmp),self.data["current"])
            folder=run/"sources"/"interview-clean"
            view=json.loads((folder/"view.json").read_text()); view["rejected"]=2; view.pop("accepted",None); view["rejected_summary"]=[{"reasons":["context dependency"]}]
            (folder/"view.json").write_text(json.dumps(view))
            selection={"selector":"local","accepted":[{"rank":1},{"rank":2},{"rank":3}],"rejected":[{"reasons":["duplicate interval"]},{"reasons":["context dependency"]}]}
            (folder/"selection.json").write_text(json.dumps(selection))
            report=mod.aggregate(run)
        source=report["sources"][0]
        self.assertEqual(source["accepted_candidates"],3)
        self.assertEqual(source["rejected_candidates"],2)
        self.assertEqual(source["duplicate_rejections"],1)
    def test_invalid_score_is_excluded(self):
        with tempfile.TemporaryDirectory() as tmp:
            path=Path(tmp)/"rubric.csv"; path.write_text("source,hook_1to5,would_post_1to5\nclip,9,5\n")
            result=mod.load_rubric(path)
        self.assertEqual(result["clips_reviewed"],0); self.assertEqual(result["invalid_rows"],[2])

if __name__=="__main__": unittest.main()
