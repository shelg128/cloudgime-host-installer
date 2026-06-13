using System.Text.Json;
using System.Text.Json.Serialization;
using Nefarius.ViGEm.Client;
using Nefarius.ViGEm.Client.Targets;
using Nefarius.ViGEm.Client.Targets.DualShock4;
using Nefarius.ViGEm.Client.Targets.Xbox360;

namespace GamepadSidecar;

internal enum GamepadProfile
{
    Xbox,
    Ds4,
}

internal sealed class BrokerCommand
{
    [JsonPropertyName("type")]
    public string Type { get; set; } = string.Empty;

    [JsonPropertyName("id")]
    public byte Id { get; set; }

    [JsonPropertyName("profile")]
    public string? Profile { get; set; }

    [JsonPropertyName("buttons")]
    public uint Buttons { get; set; }

    [JsonPropertyName("left_trigger")]
    public byte LeftTrigger { get; set; }

    [JsonPropertyName("right_trigger")]
    public byte RightTrigger { get; set; }

    [JsonPropertyName("left_stick_x")]
    public short LeftStickX { get; set; }

    [JsonPropertyName("left_stick_y")]
    public short LeftStickY { get; set; }

    [JsonPropertyName("right_stick_x")]
    public short RightStickX { get; set; }

    [JsonPropertyName("right_stick_y")]
    public short RightStickY { get; set; }
}

internal static class ControllerButtons
{
    public const uint Up = 0x0001;
    public const uint Down = 0x0002;
    public const uint Left = 0x0004;
    public const uint Right = 0x0008;
    public const uint Play = 0x0010;
    public const uint Back = 0x0020;
    public const uint LeftThumb = 0x0040;
    public const uint RightThumb = 0x0080;
    public const uint LeftShoulder = 0x0100;
    public const uint RightShoulder = 0x0200;
    public const uint Special = 0x0400;
    public const uint A = 0x1000;
    public const uint B = 0x2000;
    public const uint X = 0x4000;
    public const uint Y = 0x8000;
    public const uint Touchpad = 0x100000;
    public const uint Misc = 0x200000;
}

internal sealed class ControllerSlot : IDisposable
{
    private const int InitialNeutralWindowMs = 400;
    private const int InitialStickDeadzone = 6000;
    private readonly ViGEmClient _client;
    private IXbox360Controller? _xbox;
    private IDualShock4Controller? _ds4;
    private DateTime _connectedAtUtc = DateTime.MinValue;

    public ControllerSlot(ViGEmClient client, byte id, GamepadProfile profile)
    {
        _client = client;
        Id = id;
        Profile = profile;
        Connect(profile);
    }

    public byte Id { get; }

    public GamepadProfile Profile { get; private set; }

    public void Connect(GamepadProfile profile)
    {
        Disconnect();
        Profile = profile;

        if (profile == GamepadProfile.Ds4)
        {
            var controller = _client.CreateDualShock4Controller();
            controller.AutoSubmitReport = false;
            controller.Connect();
            _ds4 = controller;
            _connectedAtUtc = DateTime.UtcNow;
            SubmitNeutralDs4(controller);
            Console.Error.WriteLine($"[GamepadSidecar] controller {Id} connected as DS4");
        }
        else
        {
            var controller = _client.CreateXbox360Controller();
            controller.AutoSubmitReport = false;
            controller.Connect();
            _xbox = controller;
            _connectedAtUtc = DateTime.UtcNow;
            SubmitNeutralXbox(controller);
            Console.Error.WriteLine($"[GamepadSidecar] controller {Id} connected as X360");
        }
    }

    public void ApplyState(BrokerCommand command)
    {
        if (Profile == GamepadProfile.Ds4)
        {
            ApplyDs4(command);
        }
        else
        {
            ApplyXbox(command);
        }
    }

    public void Disconnect()
    {
        try
        {
            _xbox?.Disconnect();
        }
        catch
        {
        }

        try
        {
            _ds4?.Disconnect();
        }
        catch
        {
        }

        _xbox = null;
        _ds4 = null;
    }

    private void ApplyXbox(BrokerCommand command)
    {
        var controller = _xbox;
        if (controller == null)
        {
            return;
        }

        command = SanitizeInitialNeutralState(command);

        controller.ResetReport();
        controller.SetButtonState(Xbox360Button.Up, Has(command.Buttons, ControllerButtons.Up));
        controller.SetButtonState(Xbox360Button.Down, Has(command.Buttons, ControllerButtons.Down));
        controller.SetButtonState(Xbox360Button.Left, Has(command.Buttons, ControllerButtons.Left));
        controller.SetButtonState(Xbox360Button.Right, Has(command.Buttons, ControllerButtons.Right));
        controller.SetButtonState(Xbox360Button.Start, Has(command.Buttons, ControllerButtons.Play));
        controller.SetButtonState(Xbox360Button.Back, Has(command.Buttons, ControllerButtons.Back));
        controller.SetButtonState(Xbox360Button.LeftThumb, Has(command.Buttons, ControllerButtons.LeftThumb));
        controller.SetButtonState(Xbox360Button.RightThumb, Has(command.Buttons, ControllerButtons.RightThumb));
        controller.SetButtonState(Xbox360Button.LeftShoulder, Has(command.Buttons, ControllerButtons.LeftShoulder));
        controller.SetButtonState(Xbox360Button.RightShoulder, Has(command.Buttons, ControllerButtons.RightShoulder));
        controller.SetButtonState(Xbox360Button.Guide, Has(command.Buttons, ControllerButtons.Special));
        controller.SetButtonState(Xbox360Button.A, Has(command.Buttons, ControllerButtons.A));
        controller.SetButtonState(Xbox360Button.B, Has(command.Buttons, ControllerButtons.B));
        controller.SetButtonState(Xbox360Button.X, Has(command.Buttons, ControllerButtons.X));
        controller.SetButtonState(Xbox360Button.Y, Has(command.Buttons, ControllerButtons.Y));
        controller.SetSliderValue(Xbox360Slider.LeftTrigger, command.LeftTrigger);
        controller.SetSliderValue(Xbox360Slider.RightTrigger, command.RightTrigger);
        controller.SetAxisValue(Xbox360Axis.LeftThumbX, command.LeftStickX);
        controller.SetAxisValue(Xbox360Axis.LeftThumbY, InvertStick(command.LeftStickY));
        controller.SetAxisValue(Xbox360Axis.RightThumbX, command.RightStickX);
        controller.SetAxisValue(Xbox360Axis.RightThumbY, InvertStick(command.RightStickY));
        controller.SubmitReport();
    }

    private void ApplyDs4(BrokerCommand command)
    {
        var controller = _ds4;
        if (controller == null)
        {
            return;
        }

        command = SanitizeInitialNeutralState(command);

        controller.ResetReport();
        controller.SetDPadDirection(MapDs4Dpad(command.Buttons));
        controller.SetButtonState(DualShock4Button.Share, Has(command.Buttons, ControllerButtons.Back) || Has(command.Buttons, ControllerButtons.Misc));
        controller.SetButtonState(DualShock4Button.Options, Has(command.Buttons, ControllerButtons.Play));
        controller.SetButtonState(DualShock4Button.ThumbLeft, Has(command.Buttons, ControllerButtons.LeftThumb));
        controller.SetButtonState(DualShock4Button.ThumbRight, Has(command.Buttons, ControllerButtons.RightThumb));
        controller.SetButtonState(DualShock4Button.ShoulderLeft, Has(command.Buttons, ControllerButtons.LeftShoulder));
        controller.SetButtonState(DualShock4Button.ShoulderRight, Has(command.Buttons, ControllerButtons.RightShoulder));
        controller.SetButtonState(DualShock4Button.Square, Has(command.Buttons, ControllerButtons.X));
        controller.SetButtonState(DualShock4Button.Cross, Has(command.Buttons, ControllerButtons.A));
        controller.SetButtonState(DualShock4Button.Circle, Has(command.Buttons, ControllerButtons.B));
        controller.SetButtonState(DualShock4Button.Triangle, Has(command.Buttons, ControllerButtons.Y));
        controller.SetButtonState(DualShock4SpecialButton.Ps, Has(command.Buttons, ControllerButtons.Special));
        controller.SetButtonState(DualShock4SpecialButton.Touchpad, Has(command.Buttons, ControllerButtons.Touchpad));
        controller.SetSliderValue(DualShock4Slider.LeftTrigger, command.LeftTrigger);
        controller.SetSliderValue(DualShock4Slider.RightTrigger, command.RightTrigger);
        controller.SetAxisValue(DualShock4Axis.LeftThumbX, NormalizeStick(command.LeftStickX));
        controller.SetAxisValue(DualShock4Axis.LeftThumbY, NormalizeStick(InvertStick(command.LeftStickY)));
        controller.SetAxisValue(DualShock4Axis.RightThumbX, NormalizeStick(command.RightStickX));
        controller.SetAxisValue(DualShock4Axis.RightThumbY, NormalizeStick(InvertStick(command.RightStickY)));
        controller.SubmitReport();
    }

    private static void SubmitNeutralXbox(IXbox360Controller controller)
    {
        controller.ResetReport();
        controller.SetButtonState(Xbox360Button.Up, false);
        controller.SetButtonState(Xbox360Button.Down, false);
        controller.SetButtonState(Xbox360Button.Left, false);
        controller.SetButtonState(Xbox360Button.Right, false);
        controller.SetButtonState(Xbox360Button.Start, false);
        controller.SetButtonState(Xbox360Button.Back, false);
        controller.SetButtonState(Xbox360Button.LeftThumb, false);
        controller.SetButtonState(Xbox360Button.RightThumb, false);
        controller.SetButtonState(Xbox360Button.LeftShoulder, false);
        controller.SetButtonState(Xbox360Button.RightShoulder, false);
        controller.SetButtonState(Xbox360Button.Guide, false);
        controller.SetButtonState(Xbox360Button.A, false);
        controller.SetButtonState(Xbox360Button.B, false);
        controller.SetButtonState(Xbox360Button.X, false);
        controller.SetButtonState(Xbox360Button.Y, false);
        controller.SetSliderValue(Xbox360Slider.LeftTrigger, 0);
        controller.SetSliderValue(Xbox360Slider.RightTrigger, 0);
        controller.SetAxisValue(Xbox360Axis.LeftThumbX, 0);
        controller.SetAxisValue(Xbox360Axis.LeftThumbY, 0);
        controller.SetAxisValue(Xbox360Axis.RightThumbX, 0);
        controller.SetAxisValue(Xbox360Axis.RightThumbY, 0);
        controller.SubmitReport();
    }

    private static void SubmitNeutralDs4(IDualShock4Controller controller)
    {
        controller.ResetReport();
        controller.SetDPadDirection(DualShock4DPadDirection.None);
        controller.SetButtonState(DualShock4Button.Share, false);
        controller.SetButtonState(DualShock4Button.Options, false);
        controller.SetButtonState(DualShock4Button.ThumbLeft, false);
        controller.SetButtonState(DualShock4Button.ThumbRight, false);
        controller.SetButtonState(DualShock4Button.ShoulderLeft, false);
        controller.SetButtonState(DualShock4Button.ShoulderRight, false);
        controller.SetButtonState(DualShock4Button.Square, false);
        controller.SetButtonState(DualShock4Button.Cross, false);
        controller.SetButtonState(DualShock4Button.Circle, false);
        controller.SetButtonState(DualShock4Button.Triangle, false);
        controller.SetButtonState(DualShock4SpecialButton.Ps, false);
        controller.SetButtonState(DualShock4SpecialButton.Touchpad, false);
        controller.SetSliderValue(DualShock4Slider.LeftTrigger, 0);
        controller.SetSliderValue(DualShock4Slider.RightTrigger, 0);
        controller.SetAxisValue(DualShock4Axis.LeftThumbX, 128);
        controller.SetAxisValue(DualShock4Axis.LeftThumbY, 128);
        controller.SetAxisValue(DualShock4Axis.RightThumbX, 128);
        controller.SetAxisValue(DualShock4Axis.RightThumbY, 128);
        controller.SubmitReport();
    }

    private static DualShock4DPadDirection MapDs4Dpad(uint buttons)
    {
        var up = Has(buttons, ControllerButtons.Up);
        var down = Has(buttons, ControllerButtons.Down);
        var left = Has(buttons, ControllerButtons.Left);
        var right = Has(buttons, ControllerButtons.Right);

        if (up && right) return DualShock4DPadDirection.Northeast;
        if (right && down) return DualShock4DPadDirection.Southeast;
        if (down && left) return DualShock4DPadDirection.Southwest;
        if (left && up) return DualShock4DPadDirection.Northwest;
        if (up) return DualShock4DPadDirection.North;
        if (right) return DualShock4DPadDirection.East;
        if (down) return DualShock4DPadDirection.South;
        if (left) return DualShock4DPadDirection.West;
        return DualShock4DPadDirection.None;
    }

    private static byte NormalizeStick(short value)
    {
        var normalized = ((value + 32768.0) * 255.0) / 65535.0;
        var rounded = (int)Math.Round(normalized);
        return (byte)Math.Clamp(rounded, 0, 255);
    }

    private static short InvertStick(short value)
    {
        if (value == short.MinValue)
        {
            return short.MaxValue;
        }

        return (short)-value;
    }

    private static bool Has(uint bits, uint flag) => (bits & flag) != 0;

    private BrokerCommand SanitizeInitialNeutralState(BrokerCommand command)
    {
        if ((DateTime.UtcNow - _connectedAtUtc).TotalMilliseconds > InitialNeutralWindowMs)
        {
            return command;
        }

        if (command.Buttons != 0 || command.LeftTrigger != 0 || command.RightTrigger != 0)
        {
            return command;
        }

        if (!IsNearNeutral(command.LeftStickX) ||
            !IsNearNeutral(command.LeftStickY) ||
            !IsNearNeutral(command.RightStickX) ||
            !IsNearNeutral(command.RightStickY))
        {
            Console.Error.WriteLine(
                $"[GamepadSidecar] suppressing initial stray axes id={Id} lx={command.LeftStickX} ly={command.LeftStickY} rx={command.RightStickX} ry={command.RightStickY}");
            return new BrokerCommand
            {
                Type = command.Type,
                Id = command.Id,
                Profile = command.Profile,
                Buttons = command.Buttons,
                LeftTrigger = command.LeftTrigger,
                RightTrigger = command.RightTrigger,
                LeftStickX = 0,
                LeftStickY = 0,
                RightStickX = 0,
                RightStickY = 0,
            };
        }

        return command;
    }

    private static bool IsNearNeutral(short value) => Math.Abs((int)value) <= InitialStickDeadzone;

    public void Dispose()
    {
        Disconnect();
    }
}

internal static class Program
{
    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNameCaseInsensitive = true,
    };

    public static int Main()
    {
        try
        {
            using var client = new ViGEmClient();
            var slots = new Dictionary<byte, ControllerSlot>();

            Console.Error.WriteLine("[GamepadSidecar] ready");

            string? line;
            while ((line = Console.ReadLine()) != null)
            {
                if (string.IsNullOrWhiteSpace(line))
                {
                    continue;
                }

                BrokerCommand? command;
                try
                {
                    command = JsonSerializer.Deserialize<BrokerCommand>(line, JsonOptions);
                }
                catch (Exception ex)
                {
                    Console.Error.WriteLine($"[GamepadSidecar] invalid json: {ex.Message}");
                    continue;
                }

                if (command == null || string.IsNullOrWhiteSpace(command.Type))
                {
                    continue;
                }

                switch (command.Type)
                {
                    case "connect":
                    {
                        var profile = string.Equals(command.Profile, "ds4", StringComparison.OrdinalIgnoreCase)
                            ? GamepadProfile.Ds4
                            : GamepadProfile.Xbox;
                        if (slots.TryGetValue(command.Id, out var existing))
                        {
                            if (existing.Profile != profile)
                            {
                                existing.Dispose();
                                slots.Remove(command.Id);
                            }
                        }

                        if (!slots.TryGetValue(command.Id, out existing))
                        {
                            existing = new ControllerSlot(client, command.Id, profile);
                            slots[command.Id] = existing;
                        }
                        else
                        {
                            existing.Connect(profile);
                        }

                        break;
                    }
                    case "disconnect":
                    {
                        if (slots.Remove(command.Id, out var slot))
                        {
                            slot.Dispose();
                            Console.Error.WriteLine($"[GamepadSidecar] controller {command.Id} disconnected");
                        }
                        break;
                    }
                    case "state":
                    {
                        if (slots.TryGetValue(command.Id, out var slot))
                        {
                            slot.ApplyState(command);
                        }
                        break;
                    }
                    case "stop":
                    {
                        foreach (var slot in slots.Values)
                        {
                            slot.Dispose();
                        }
                        slots.Clear();
                        return 0;
                    }
                }
            }

            foreach (var slot in slots.Values)
            {
                slot.Dispose();
            }
            return 0;
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"[GamepadSidecar] fatal: {ex}");
            return 1;
        }
    }
}
