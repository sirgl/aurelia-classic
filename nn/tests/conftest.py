import os
import sys
import warnings

NN = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if NN not in sys.path:
    sys.path.insert(0, NN)

# Shards are memory-mapped read-only; torch warns when it wraps them.
warnings.filterwarnings("ignore", message="The given NumPy array is not writable")
