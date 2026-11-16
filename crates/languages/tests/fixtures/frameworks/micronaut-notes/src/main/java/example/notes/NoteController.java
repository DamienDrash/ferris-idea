package example.notes;

import io.micronaut.context.annotation.Property;
import io.micronaut.context.annotation.Value;
import io.micronaut.http.HttpStatus;
import io.micronaut.http.MediaType;
import io.micronaut.http.annotation.Body;
import io.micronaut.http.annotation.Controller;
import io.micronaut.http.annotation.Delete;
import io.micronaut.http.annotation.Get;
import io.micronaut.http.annotation.Head;
import io.micronaut.http.annotation.Patch;
import io.micronaut.http.annotation.Post;
import io.micronaut.http.annotation.Produces;
import io.micronaut.http.annotation.Put;
import io.micronaut.http.annotation.Status;
import jakarta.inject.Inject;
import jakarta.inject.Named;
import java.util.List;

@Controller("/notes")
public class NoteController {

    private final NoteService service;

    // Defined in application.yml.
    @Value("${notes.title}")
    String title;

    // Kebab-case key, written in camel case in application.yml.
    @Value("${notes.page-size}")
    int pageSize;

    // Defined nowhere, with a default.
    @Value("${notes.footer:none}")
    String footer;

    // Defined nowhere.
    @Value("${notes.missing}")
    String missing;

    @Property(name = "notes.owner")
    String owner;

    // Ambiguous: two exporters.
    @Inject
    Exporter exporter;

    @Inject
    @Named("json")
    Exporter json;

    // Missing: no scope annotation, no factory.
    @Inject
    Archive archive;

    // A configuration properties class and a declarative client: both are beans.
    @Inject
    NotesConfiguration configuration;

    @Inject
    SpellClient spell;

    // A single constructor needs no @Inject.
    public NoteController(NoteService service) {
        this.service = service;
    }

    @Get
    public List<Note> list() {
        return service.all();
    }

    @Get("/{id}")
    public Note one(long id) {
        return service.all().get(0);
    }

    @Get(uri = "/export", produces = MediaType.TEXT_PLAIN)
    public String export() {
        return exporter.export(service.all());
    }

    @Get(produces = MediaType.TEXT_PLAIN, uri = "/count")
    public String count() {
        return "0";
    }

    @Post
    @Status(HttpStatus.CREATED)
    public Note add(@Body Note note) {
        return note;
    }

    @Put("/{id}")
    public Note replace(long id, @Body Note note) {
        return note;
    }

    @Patch("/{id}")
    @Produces(MediaType.APPLICATION_JSON)
    public Note change(long id, @Body Note note) {
        return note;
    }

    @Delete("/{id}")
    public void remove(long id) {
    }

    @Head("/{id}")
    public void exists(long id) {
    }
}
