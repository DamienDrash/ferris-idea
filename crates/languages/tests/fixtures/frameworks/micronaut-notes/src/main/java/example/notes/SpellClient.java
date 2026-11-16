package example.notes;

import io.micronaut.http.annotation.Get;
import io.micronaut.http.client.annotation.Client;

// A declarative client, not an endpoint of this application.
@Client("https://spell.example")
public interface SpellClient {

    @Get("/check/{word}")
    String check(String word);
}
