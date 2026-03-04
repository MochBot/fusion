"""Neural network model definitions for fusion-engine coaching."""

from .student import StudentNet
from .teacher import TeacherNet

__all__ = ["StudentNet", "TeacherNet"]
