// Fixture: methods, constructors, local functions, lambdas, small bodies.

public class Ledger
{
    private readonly Dictionary<string, decimal> balances = new();

    public Ledger(IEnumerable<(string, decimal)> seed)
    {
        // seed the initial balances
        foreach (var (name, amount) in seed)
        {
            balances[name] = balances.GetValueOrDefault(name) + amount;
        }
    }

    public decimal Transfer(string from, string to, decimal amount)
    {
        decimal Withdraw(string account)
        {
            var available = balances.GetValueOrDefault(account);
            var taken = Math.Min(available, amount);
            balances[account] = available - taken;
            return taken;
        }

        var moved = Withdraw(from);
        balances[to] = balances.GetValueOrDefault(to) + moved;
        return moved;
    }

    public int Tiny()
    {
        return 1;
    }
}
