#include <algorithm>
#include <string>
#include <vector>

namespace demo {

class Store {
public:
    int merge(std::vector<int> values) {
        int added = 0;
        for (int value : values) {
            if (std::find(items.begin(), items.end(), value) == items.end()) {
                items.push_back(value);
                added++;
            }
        }
        return added;
    }

private:
    std::vector<int> items;
};

int summarize(const std::vector<std::string>& names) {
    int total = 0;
    auto count_long = [&total](const std::string& name) {
        if (name.size() > 3) {
            total += static_cast<int>(name.size());
            return true;
        }
        return false;
    };
    for (const auto& name : names) {
        count_long(name);
    }
    return total;
}

}
