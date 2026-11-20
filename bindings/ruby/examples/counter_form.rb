require "grafikr"

Grafikr::App.open(host: ENV["GRAFIKR_HOST"]) do |app|
  window = app.create_window("Counter and form", 480, 320)
  label = app.create_node(window, "Text", text: "Count: 0"); label.attach(window.root)
  button = app.create_node(window, "Button", label: "Add"); button.attach(window.root)
  app.on(button.handle, "click") { label.update(text: "Count: 1") }
  sleep
end
