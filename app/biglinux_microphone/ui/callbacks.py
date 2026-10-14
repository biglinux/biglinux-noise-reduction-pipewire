"""Weak owner callbacks for widgets, controllers and drawing functions.

GTK owns these callbacks outside Python's object graph. Capturing an ancestor
strongly can therefore retain an entire closed window, even after gc.collect().
The view/page owns its widgets; their callbacks only borrow that owner.
"""

from collections.abc import Callable
from weakref import ref


def weak_callback(owner: object, callback: Callable) -> Callable:
    """Call an unbound function with its live owner, or return False after teardown."""
    owner_ref = ref(owner)

    def invoke(*args):
        current = owner_ref()
        if current is None:
            return False
        return callback(current, *args)

    return invoke
