package example.notes;

import jakarta.inject.Named;
import jakarta.inject.Singleton;
import java.util.List;

@Singleton
@Named("json")
public class JsonExporter implements Exporter {
    @Override
    public String export(List<Note> notes) {
        return "[]";
    }
}
