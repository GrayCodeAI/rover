# Python CLI client

`rover_client.py` requires only Python's standard library. It invokes your explicitly
selected Rover binary with argv, never a shell. No PyPI package is claimed.

```python
from rover_client import Rover
client = Rover("/absolute/path/to/rover", "/absolute/path/to/private-state")
result = client.status()
print(result.exit_code, result.data)
```

Task submission exit zero means admitted, not accepted software. Verification
codes 1/2/3 remain ordinary structured `Result`s rather than being hidden by an
exception. A client timeout may leave an admitted detached task running. Reconcile
before retrying. External installations and credentials are never automatic.

Run tests: `python3 -m unittest discover -s sdk/python -p 'test_*.py'`.
