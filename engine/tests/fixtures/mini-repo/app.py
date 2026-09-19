class Service:
    """A small service used by the smoke fixture."""

    def run(self, value):
        return helper(value)


def helper(value):
    return value + 1
