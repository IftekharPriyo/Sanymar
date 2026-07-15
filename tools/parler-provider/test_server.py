import unittest

from server import delivery_description, validate_request


class ProviderContractTests(unittest.TestCase):
    def test_energetic_description_preserves_speaker_and_adds_broadcast_energy(self) -> None:
        description = delivery_description("Jon", "energetic", 1.0)
        self.assertIn("Jon", description)
        self.assertIn("confident emphasis", description)
        self.assertIn("slightly quick broadcast pace", description)
        self.assertIn("smooth and connected", description)
        self.assertIn("no vocal distortion", description)
        self.assertIn("background noise", description)

    def test_request_rejects_unknown_fields_and_reference_audio(self) -> None:
        valid = {
            "text": "A short line.",
            "speaker": "Jon",
            "deliveryStyle": "energetic",
            "rate": 1.0,
            "volume": 1.0,
        }
        self.assertEqual(validate_request(valid)[0], "A short line.")
        with self.assertRaises(ValueError):
            validate_request({**valid, "referenceAudio": "not allowed"})

    def test_request_rejects_unbounded_values(self) -> None:
        with self.assertRaises(ValueError):
            validate_request(
                {
                    "text": "A short line.",
                    "speaker": "Jon",
                    "deliveryStyle": "energetic",
                    "rate": 3.0,
                    "volume": 1.0,
                }
            )


if __name__ == "__main__":
    unittest.main()
