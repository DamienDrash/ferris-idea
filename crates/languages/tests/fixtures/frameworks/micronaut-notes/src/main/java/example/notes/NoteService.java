package example.notes;

import jakarta.inject.Singleton;
import java.time.Clock;
import java.util.List;

@Singleton
public class NoteService {
    private final Clock clock;

    public NoteService(Clock clock) {
        this.clock = clock;
    }

    public List<Note> all() {
        return List.of();
    }
}
