"""Publish the JVM SDK to a temporary Maven repository and test clean consumers."""

from __future__ import annotations

import argparse
import json
import re
import tempfile
from pathlib import Path
import hashlib
import zipfile
import os

from scripts.build_runtime import ROOT, _tool, install_bundle, validate_manifest

WRAPPER = ROOT / "bindings/jvm/gradle/wrapper/gradle-wrapper.jar"
PROJECT = ROOT / "bindings/jvm/build.gradle.kts"
RUNTIME_CLASSIFIERS = {
    "x86_64-pc-windows-msvc": "windows-x64",
    "aarch64-pc-windows-msvc": "windows-arm64",
    "x86_64-unknown-linux-gnu": "linux-x64",
    "aarch64-unknown-linux-gnu": "linux-arm64",
    "x86_64-apple-darwin": "macos-x64",
    "aarch64-apple-darwin": "macos-arm64",
}


def project_version(project: Path = PROJECT) -> str:
    match = re.search(r'(?m)^version = "([^"]+)"$', project.read_text(encoding="utf-8"))
    if match is None:
        raise ValueError("JVM Gradle project does not declare a package version")
    return match.group(1)


def package_runtime(bundle: Path, repository: Path) -> Path:
    bundle = bundle.resolve()
    manifest = validate_manifest(bundle)
    try:
        classifier = RUNTIME_CLASSIFIERS[manifest["target"]]
    except KeyError as error:
        raise ValueError(f"unsupported JVM runtime target: {manifest['target']}") from error
    repository = repository.resolve()
    (ROOT / "target").mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="grafikr-jvm-runtime-", dir=ROOT / "target") as temporary:
        staged_bundle = install_bundle(bundle, Path(temporary) / "runtime")
        _tool(
            "java", "-classpath", str(WRAPPER), "org.gradle.wrapper.GradleWrapperMain",
            "-p", str(ROOT / "bindings/jvm"), "publishRuntimePublicationToLocalRepository",
            "--no-daemon", f"-PmavenRepoPath={repository}",
            f"-PgrafikrRuntimeBundle={staged_bundle}", f"-PgrafikrRuntimeClassifier={classifier}",
            cwd=ROOT,
        )
    version = project_version()
    artifact = repository / "dev/grafikr/grafikr-runtime" / version / f"grafikr-runtime-{version}-{classifier}.jar"
    if not artifact.is_file():
        raise RuntimeError(f"Gradle did not create JVM runtime artifact: {artifact.name}")
    with zipfile.ZipFile(artifact) as archive:
        packed_manifest = json.loads(archive.read("runtime/manifest.json"))
        if packed_manifest != manifest:
            raise ValueError("JVM runtime artifact manifest differs from verified bundle")
        for entry in manifest["files"]:
            contents = archive.read(f"runtime/{entry['path']}")
            if len(contents) != entry["size"] or hashlib.sha256(contents).hexdigest() != entry["sha256"]:
                raise ValueError(f"JVM runtime artifact checksum mismatch: {entry['path']}")
    return artifact


def verify(bundle: Path, output: Path | None = None, fixture_scenarios: bool = False) -> None:
    bundle = bundle.resolve()
    version = project_version()
    manifest = validate_manifest(bundle)
    if manifest["development_fixture"]:
        raise ValueError("development fixture bundles cannot be used by Maven consumers")
    host_entry = next((entry for entry in manifest["files"] if entry["role"] == "host"), None)
    if host_entry is None:
        raise ValueError("runtime manifest does not name a host executable")
    host = str(bundle / Path(host_entry["path"]))
    fixture_host = os.environ.get("GRAFIKR_FIXTURE_HOST") if fixture_scenarios else None
    if fixture_scenarios and (not fixture_host or not Path(fixture_host).is_absolute() or not Path(fixture_host).is_file()):
        raise ValueError("installed fixture scenarios require an absolute GRAFIKR_FIXTURE_HOST")
    if fixture_scenarios:
        os.environ["GRAFIKR_FIXTURE_SCENARIOS"] = "1"
    (ROOT / "target").mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="grafikr-jvm-consumer-", dir=ROOT / "target") as temporary:
        work = Path(temporary)
        repository = output.resolve() if output else work / "maven-repository"
        _tool("java", "-classpath", str(WRAPPER), "org.gradle.wrapper.GradleWrapperMain", "-p", str(ROOT / "bindings/jvm"), "publishGrafikrPublicationToLocalRepository", "--no-daemon", f"-PmavenRepoPath={repository}", cwd=ROOT)
        for language in ("java", "kotlin"):
            consumer = work / language
            source_dir = consumer / ("src/main/java" if language == "java" else "src/main/kotlin")
            source_dir.mkdir(parents=True)
            if language == "java":
                (consumer / "settings.gradle.kts").write_text('rootProject.name = "grafikr-java-consumer"\n', encoding="utf-8")
                build = f"""plugins {{ application }}
repositories {{ maven {{ url = uri({json.dumps(str(repository))}) }}; mavenCentral() }}
dependencies {{ implementation("dev.grafikr:grafikr-jvm:{version}") }}
application {{ mainClass.set("Consumer") }}
"""
                source = f"""import com.google.gson.JsonObject;
import dev.grafikr.App;
import dev.grafikr.State;
import java.nio.file.Path;
public class Consumer {{
  public static void main(String[] args) throws Exception {{
    try (App app = new App(Path.of({json.dumps(host)}))) {{
      JsonObject value = new JsonObject(); value.addProperty("type", "i64"); value.addProperty("value", Long.toString(Long.MIN_VALUE));
      State state = app.createState(value);
      if (!Long.toString(Long.MIN_VALUE).equals(state.get().get("value").getAsString())) throw new IllegalStateException("i64 minimum mismatch");
      value.addProperty("value", Long.toString(Long.MAX_VALUE)); state.set(value);
      if (!Long.toString(Long.MAX_VALUE).equals(state.get().get("value").getAsString())) throw new IllegalStateException("i64 maximum mismatch");
      state.close();
    }}
    if ("1".equals(System.getenv("GRAFIKR_FIXTURE_SCENARIOS"))) Scenario.run(Path.of(System.getenv("GRAFIKR_FIXTURE_HOST")));
  }}
}}
"""
                scenario = '''import com.google.gson.*;
import dev.grafikr.*;
import java.nio.file.Path;
import java.util.Base64;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
final class Scenario {
  static JsonObject i64(long value) { JsonObject out=new JsonObject(); out.addProperty("type","i64"); out.addProperty("value",Long.toString(value)); return out; }
  static JsonObject obj() { return new JsonObject(); }
  static JsonObject props(Object... values) { JsonObject out=new JsonObject(); for(int i=0;i<values.length;i+=2) out.add(values[i].toString(),new Gson().toJsonTree(values[i+1])); return out; }
  static void run(Path host) throws Exception {
    try (App app=new App(host)) {
      Window counter=app.createWindow("Installed counter",480,320);
      State count=app.createState(i64(0));
      Node button=app.createNode(counter,"Button",props("label","Increment"));
      Node label=app.createNode(counter,"Text",props("text","Count: 0"));
      Node field=app.createNode(counter,"TextField",props("text","Grafikr 雪","width",240));
      button.attach(counter.root()); label.attach(counter.root()); field.attach(counter.root());
      CountDownLatch clicked=new CountDownLatch(1);
      Subscription subscription=app.on(button.handle(),"click",event -> {
        try {
          long next=Long.parseLong(count.get().get("value").getAsString())+1;
          count.set(i64(next));
          label.update(props("text","Count: "+next));
          clicked.countDown();
        } catch(Exception error) { throw new RuntimeException(error); }
      });
      if(!clicked.await(5,TimeUnit.SECONDS)) throw new IllegalStateException("counter click timed out");
      if(!count.get().get("value").getAsString().equals("1")) throw new IllegalStateException("counter state mismatch");
      field.update(props("text","Input 雪"));
      subscription.close(); count.close(); counter.close();

      Window dashboard=app.createWindow("Installed dashboard",640,480);
      Node rows=app.createNode(dashboard,"VirtualList",props("row_count",50000,"row_height",24,"width",600,"height",360));
      rows.attach(dashboard.root());
      try(CollectionProvider provider=CollectionProvider.create(rows,50000,24,"label")) {
        CollectionRequest request=provider.nextRequest(5,TimeUnit.SECONDS);
        if(request==null||request.start()!=0||request.count()!=32) throw new IllegalStateException("visible range mismatch");
        JsonArray data=new JsonArray();
        for(int i=0;i<request.count();i++){JsonArray row=new JsonArray();row.add("Row "+i);data.add(row);}
        provider.provide(request,data);
      }
      JsonArray empty=new JsonArray();
      JsonObject canvasProps=props("label","Dashboard canvas","width",320,"height",120);canvasProps.add("commands",empty);
      Node canvas=app.createNode(dashboard,"Canvas",canvasProps);
      canvas.attach(dashboard.root());
      JsonArray commands=new JsonArray();JsonObject command=new JsonObject();command.addProperty("kind","rect");
      command.add("rect",props("x",0,"y",0,"width",32,"height",24));
      command.addProperty("color","#336699");command.addProperty("radius",0);commands.add(command);
      JsonObject update=new JsonObject();update.addProperty("node",canvas.handle());update.add("commands",commands);app.request("canvas.update",update);
      JsonObject begin=new JsonObject();begin.addProperty("width",1);begin.addProperty("height",1);begin.addProperty("byte_length",4);
      String upload=app.request("image.begin",begin).get("upload").getAsString();
      JsonObject chunk=new JsonObject();chunk.addProperty("upload",upload);chunk.addProperty("offset",0);chunk.addProperty("data",Base64.getEncoder().encodeToString(new byte[]{0,0,0,(byte)255}));app.request("image.chunk",chunk);
      JsonObject finalize=new JsonObject();finalize.addProperty("upload",upload);String image=app.request("image.finalize",finalize).get("image").getAsString();
      Node imageNode=app.createNode(dashboard,"Image",props("image",image,"width",1,"height",1,"label","Pixel"));
      imageNode.attach(dashboard.root());JsonObject destroyImage=new JsonObject();destroyImage.addProperty("image",image);app.request("image.destroy",destroyImage);
      imageNode.destroy();canvas.destroy();rows.destroy();dashboard.close();

      Window first=app.createWindow("First window",320,240), second=app.createWindow("Second window",320,240);
      Node modal=app.createNode(second,"Modal",props("open",true,"width",240));
      modal.attach(second.root());Node message=app.createNode(second,"Text",props("text","Confirm"));message.attach(modal.handle());
      State shared=app.createState(i64(1));first.close();shared.set(i64(2));
      if(!shared.get().get("value").getAsString().equals("2"))throw new IllegalStateException("remaining window lost state");
      message.destroy();modal.destroy();shared.close();second.close();
    }
  }
}
'''
                (source_dir / "Scenario.java").write_text(scenario, encoding="utf-8")
            else:
                (consumer / "settings.gradle.kts").write_text('rootProject.name = "grafikr-kotlin-consumer"\n', encoding="utf-8")
                build = f"""plugins {{ kotlin("jvm") version "2.2.0"; application }}
repositories {{ maven {{ url = uri({json.dumps(str(repository))}) }}; mavenCentral() }}
dependencies {{ implementation("dev.grafikr:grafikr-jvm:{version}") }}
application {{ mainClass.set("ConsumerKt") }}
"""
                source = f"""import com.google.gson.JsonObject
import dev.grafikr.App
import dev.grafikr.CollectionProvider
import java.nio.file.Path
fun main() {{
  App(Path.of({json.dumps(host)})).use {{ app ->
    fun value(number: Long) = JsonObject().apply {{ addProperty("type", "i64"); addProperty("value", number.toString()) }}
    val state = app.createState(value(Long.MIN_VALUE))
    check(state.get().get("value").asString == Long.MIN_VALUE.toString())
    state.set(value(Long.MAX_VALUE))
    check(state.get().get("value").asString == Long.MAX_VALUE.toString())
    state.close()
  }}
  if (System.getenv("GRAFIKR_FIXTURE_SCENARIOS") == "1") runScenarios(Path.of(System.getenv("GRAFIKR_FIXTURE_HOST")))
}}
fun runScenarios(host: Path) {{
  App(host).use {{ app ->
    fun i64(value: Long) = JsonObject().apply {{ addProperty("type", "i64"); addProperty("value", value.toString()) }}
    val counter = app.createWindow("Installed counter", 480.0, 320.0)
    val count = app.createState(i64(0))
    val buttonProps=JsonObject().apply {{ addProperty("label","Increment") }}
    val labelProps=JsonObject().apply {{ addProperty("text","Count: 0") }}
    val fieldProps=JsonObject().apply {{ addProperty("text","Grafikr 雪"); addProperty("width",240) }}
    val button=app.createNode(counter,"Button",buttonProps);val label=app.createNode(counter,"Text",labelProps);val field=app.createNode(counter,"TextField",fieldProps)
    button.attach(counter.root);label.attach(counter.root);field.attach(counter.root)
    val clicked=java.util.concurrent.CountDownLatch(1)
    val subscription=app.on(button.handle,"click") {{ _ ->
      val next=count.get().get("value").asString.toLong()+1
      count.set(i64(next));label.update(JsonObject().apply {{ addProperty("text","Count: $next") }});clicked.countDown()
    }}
    check(clicked.await(5,java.util.concurrent.TimeUnit.SECONDS));check(count.get().get("value").asString=="1")
    field.update(JsonObject().apply {{ addProperty("text","Input 雪") }});subscription.close();count.close();counter.close()

    val dashboard=app.createWindow("Installed dashboard",640.0,480.0)
    val rows=app.createNode(dashboard,"VirtualList",JsonObject().apply {{ addProperty("row_count",50000);addProperty("row_height",24);addProperty("width",600);addProperty("height",360) }})
    rows.attach(dashboard.root)
    val provider=CollectionProvider.create(rows,50000,24.0,"label")
    val request=provider.nextRequest(5,java.util.concurrent.TimeUnit.SECONDS);check(request!=null && request.start()==0L && request.count()==32)
    val data=com.google.gson.JsonArray();repeat(request!!.count()) {{ data.add(com.google.gson.JsonArray().apply {{ add("Row $it") }}) }}
    provider.provide(request,data);provider.close()
    val canvas=app.createNode(dashboard,"Canvas",JsonObject().apply {{ add("commands",com.google.gson.JsonArray());addProperty("label","Dashboard canvas");addProperty("width",320);addProperty("height",120) }})
    canvas.attach(dashboard.root)
    val commands=com.google.gson.JsonArray();commands.add(JsonObject().apply {{ addProperty("kind","rect");add("rect",JsonObject().apply {{ addProperty("x",0);addProperty("y",0);addProperty("width",32);addProperty("height",24) }});addProperty("color","#336699");addProperty("radius",0) }})
    app.request("canvas.update",JsonObject().apply {{ addProperty("node",canvas.handle);add("commands",commands) }})
    val upload=app.request("image.begin",JsonObject().apply {{ addProperty("width",1);addProperty("height",1);addProperty("byte_length",4) }}).get("upload").asString
    app.request("image.chunk",JsonObject().apply {{ addProperty("upload",upload);addProperty("offset",0);addProperty("data",java.util.Base64.getEncoder().encodeToString(byteArrayOf(0,0,0,(-1).toByte()))) }})
    val image=app.request("image.finalize",JsonObject().apply {{ addProperty("upload",upload) }}).get("image").asString
    val imageNode=app.createNode(dashboard,"Image",JsonObject().apply {{ addProperty("image",image);addProperty("width",1);addProperty("height",1);addProperty("label","Pixel") }})
    imageNode.attach(dashboard.root);app.request("image.destroy",JsonObject().apply {{ addProperty("image",image) }})
    imageNode.destroy();canvas.destroy();rows.destroy();dashboard.close()

    val first=app.createWindow("First window",320.0,240.0);val second=app.createWindow("Second window",320.0,240.0)
    val modal=app.createNode(second,"Modal",JsonObject().apply {{ addProperty("open",true);addProperty("width",240) }})
    modal.attach(second.root);val message=app.createNode(second,"Text",JsonObject().apply {{ addProperty("text","Confirm") }});message.attach(modal.handle)
    val shared=app.createState(i64(1));first.close();shared.set(i64(2));check(shared.get().get("value").asString=="2")
    message.destroy();modal.destroy();shared.close();second.close()
  }}
}}
"""
            (consumer / "build.gradle.kts").write_text(build, encoding="utf-8")
            (source_dir / ("Consumer.java" if language == "java" else "Consumer.kt")).write_text(source, encoding="utf-8")
            _tool("java", "-classpath", str(WRAPPER), "org.gradle.wrapper.GradleWrapperMain", "-p", str(consumer), "run", "--no-daemon", cwd=consumer)
    print("Java and Kotlin Maven consumers installed the local artifact and passed signed i64 IPC round trips")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--out", type=Path)
    parser.add_argument("--fixture-scenarios", action="store_true")
    args = parser.parse_args(argv)
    try:
        verify(args.bundle, args.out, args.fixture_scenarios)
    except (OSError, ValueError, RuntimeError, json.JSONDecodeError) as error:
        parser.exit(1, f"JVM package verification failed: {error}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
