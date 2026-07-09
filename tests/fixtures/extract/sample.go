// Fixture: functions, methods with receivers, func literals, small bodies.

package sample

type Store struct {
	data map[string][]byte
}

func (s *Store) Merge(other map[string][]byte) int {
	// copy entries that are not already present
	added := 0
	for key, value := range other {
		if _, exists := s.data[key]; !exists {
			s.data[key] = value
			added++
		}
	}
	return added
}

func Walk(items []string, visit func(string) error) error {
	handler := func(item string) error {
		trimmed := strings.TrimSpace(item)
		if trimmed == "" {
			return nil
		}
		return visit(trimmed)
	}
	for _, item := range items {
		if err := handler(item); err != nil {
			return err
		}
	}
	return nil
}

func MakeGreeter(prefix string) func(string) string {
	return func(name string) string {
		trimmed := strings.TrimSpace(name)
		if trimmed == "" {
			return prefix
		}
		return prefix + " " + trimmed
	}
}

func RunServer(addr string) {
	go func() {
		listener := listen(addr)
		defer listener.Close()
		serve(listener)
	}()
}

func tiny() int { return 1 }
