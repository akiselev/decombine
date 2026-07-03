"""Fixture: methods, docstrings, lambdas, nested functions, small bodies."""


class Inventory:
    def restock(self, items):
        """Restock the inventory from a list of (name, count) pairs."""
        # merge counts into the ledger
        for name, count in items:
            current = self.ledger.get(name, 0)
            self.ledger[name] = current + count
        return len(self.ledger)


def report(inventory, minimum):
    def format_line(name, count):
        label = name.upper()
        padded = label.ljust(20)
        return f"{padded} {count}"

    lines = []
    for name, count in sorted(inventory.ledger.items()):
        if count >= minimum:
            lines.append(format_line(name, count))
    return "\n".join(lines)


def tiny():
    return 1
