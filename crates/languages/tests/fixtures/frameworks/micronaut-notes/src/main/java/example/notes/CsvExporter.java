package example.notes;

import io.micronaut.context.annotation.Prototype;
import java.util.List;

@Prototype
public class CsvExporter implements Exporter {
    @Override
    public String export(List<Note> notes) {
        return "";
    }
}
