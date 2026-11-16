package org.acme;

import jakarta.ws.rs.GET;
import jakarta.ws.rs.Path;
import jakarta.ws.rs.PathParam;
import org.eclipse.microprofile.rest.client.inject.RegisterRestClient;

// A client, not an endpoint of this application.
@Path("/v3.1")
@RegisterRestClient(configKey = "countries")
public interface CountryClient {

    @GET
    @Path("/name/{name}")
    String byName(@PathParam("name") String name);
}
