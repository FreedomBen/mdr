#!/usr/bin/env ruby
# frozen_string_literal: true

require "minitest/autorun"
require "fileutils"
require "tmpdir"
require "open3"
require "net/http"
require "socket"

ROOT = File.expand_path("../..", __dir__)
BIN_PATH = File.join(ROOT, "target", "debug", "mdr")
FIXTURES_DIR = File.join(ROOT, "tests", "fixtures")
INPUT_FIXTURE = File.join(FIXTURES_DIR, "full.md")
EXPECTED_HTML = File.join(FIXTURES_DIR, "full.html")
KATEX_URL = "file://#{File.join(FIXTURES_DIR, "katex")}/"

class MdrEndToEndTest < Minitest::Test
  def setup
    assert File.executable?(BIN_PATH), "mdr binary not built; run `make build` first"
    assert File.file?(INPUT_FIXTURE), "missing markdown fixture: #{INPUT_FIXTURE}"
    assert File.file?(EXPECTED_HTML), "missing expected HTML fixture: #{EXPECTED_HTML}"
    assert pandoc_available?, "pandoc is required for end-to-end tests"
  end

  def test_builds_expected_html_with_output_flag
    Dir.mktmpdir("mdr-e2e-output") do |dir|
      input = copy_fixture(dir, INPUT_FIXTURE)
      output = File.join(dir, "full.html")

      stdout, stderr, status = Open3.capture3(base_env, BIN_PATH, "-o", output, input)
      assert status.success?, "mdr convert failed (status #{status.exitstatus}):\n#{stderr}\n#{stdout}"

      actual = File.read(output)
      expected = File.read(EXPECTED_HTML)
      assert_equal expected, actual, "converted HTML did not match expected fixture"
    end
  end

  def test_serves_generated_html_over_http
    Dir.mktmpdir("mdr-e2e-serve") do |dir|
      input = copy_fixture(dir, INPUT_FIXTURE)
      port = find_free_port

      server_log = File.join(dir, "server.log")
      server_io = File.open(server_log, "w")
      pid = Process.spawn(base_env, BIN_PATH, "--port", port.to_s, input, out: server_io, err: server_io)

      begin
        response = wait_for_http(port, pid: pid, log_path: server_log)
        assert_equal "200", response.code, "server responded with #{response.code}" if response
        expected_body = with_live_script(File.read(EXPECTED_HTML)).force_encoding("UTF-8")
        actual_body = response.body.dup.force_encoding("UTF-8")
        assert_equal expected_body, actual_body
      ensure
        Process.kill("TERM", pid) rescue nil
        Process.wait(pid) rescue nil
        server_io.close
      end
    end
  end

  private

  def base_env
    { "MDR_KATEX" => KATEX_URL }
  end

  def copy_fixture(dir, source)
    destination = File.join(dir, File.basename(source))
    FileUtils.cp(source, destination)
    destination
  end

  def pandoc_available?
    system(base_env, "pandoc", "--version", out: File::NULL, err: File::NULL)
  end

  def find_free_port
    server = TCPServer.new("127.0.0.1", 0)
    port = server.addr[1]
    server.close
    port
  end

  def wait_for_http(port, pid:, log_path:, timeout: 10)
    deadline = Process.clock_gettime(Process::CLOCK_MONOTONIC) + timeout

    loop do
      begin
        response = Net::HTTP.get_response(URI("http://127.0.0.1:#{port}/"))
        return response if response.is_a?(Net::HTTPOK)
      rescue Errno::ECONNREFUSED, Errno::EHOSTUNREACH, Errno::ECONNRESET
      end

      if (waited = Process.waitpid(pid, Process::WNOHANG))
        status = $?.dup
        log = File.exist?(log_path) ? File.read(log_path) : ""
        flunk "mdr serve exited early (pid #{waited}, status #{status})\n#{log}"
      end

      break if Process.clock_gettime(Process::CLOCK_MONOTONIC) >= deadline
      sleep 0.1
    end

    log = File.exist?(log_path) ? File.read(log_path) : ""
    flunk "server did not become ready on port #{port}\n#{log}"
  end

  def with_live_script(html)
    html.include?("/live.js") ? html : "#{html}\n<script src=\"/live.js\"></script>\n"
  end
end
