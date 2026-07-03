// Fixture: methods, constructors, lambdas, comments, small bodies.

public class Sample {
    private final List<String> entries;

    public Sample(List<String> initial) {
        // defensive copy of the seed data
        this.entries = new ArrayList<>(initial);
        this.entries.removeIf(Objects::isNull);
    }

    public Map<String, Integer> tally() {
        /* group by value and count */
        Map<String, Integer> counts = new HashMap<>();
        for (String entry : entries) {
            counts.merge(entry.trim().toLowerCase(), 1, Integer::sum);
        }
        return counts;
    }

    public int tiny() {
        return 1;
    }
}
