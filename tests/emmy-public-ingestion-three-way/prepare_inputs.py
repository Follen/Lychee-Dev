"""Fixed recipe assembly inside the already-owned Windows contract job.
No subprocesses, new downloader, Cargo, or changes to original upstream files.
"""
import importlib.util
import json
import os
from pathlib import Path
import sys
import time

HERE = Path(__file__).resolve().parent
if len(sys.argv) != 1 or os.name != 'nt':
    raise ValueError('windows-contract.windows_only_no_arguments')
job = Path.cwd().absolute()
workspace = Path(os.environ['GITHUB_WORKSPACE']).absolute()
if job.parent != workspace / '.tmp/rust-scratch' or len(job.name) != 36:
    raise ValueError('windows-contract.owned_job_path')
spec = importlib.util.spec_from_file_location('fixed_emmy_recipe', HERE / 'third_party/emmy/prepare.py')
recipe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(recipe)
recipe.check_ancestors(job)
identity, expected, patches, inputs = recipe.load_recipe()
if identity['patchedTreeSHA256'] != '5fd1358357d7480d40e6ace0884300235780c04060a503f6fc12035dd6be30d6':
    raise ValueError('windows-contract.fixed_recipe')
base = identity['base']
if base['archiveSHA256'] != 'ae529a06b551dbb301744bea2b58ba94cb3e30340f63a8c83f9335be9086fdb7':
    raise ValueError('windows-contract.fixed_archive')
deadline = time.monotonic() + 300
archive, original, product = job / 'base.tar.gz', job / 'original', job / 'product'
started = time.perf_counter_ns()
recipe.download_archive(archive, base, deadline)
download_ns = time.perf_counter_ns() - started
started = time.perf_counter_ns()
original_rows = recipe.extract_archive(archive, original, base, deadline=deadline)
recipe.publish(original, product, original_rows, deadline=deadline)
for _, patch in patches:
    recipe.apply_patch(product, patch, deadline=deadline)
recipe.verify(product, expected, deadline=deadline)
recipe.verify(original, original_rows, deadline=deadline)
recipe.verify_recipe_inputs(inputs)
print(json.dumps({'state': 'assembled', 'archiveSHA256': base['archiveSHA256'],
                  'originalFiles': len(original_rows), 'originalTreeSHA256': recipe.tree_digest(original_rows),
                  'productFiles': len(expected), 'productTreeSHA256': recipe.tree_digest(expected),
                  'downloadNs': download_ns, 'assemblyNs': time.perf_counter_ns() - started}, separators=(',', ':')))
